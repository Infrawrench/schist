//! Merge snapshots of selected open documents on the background executor.

use super::*;
use schist_core::{blit_rgba_f32, DocumentId};
use schist_i18n::t;
use schist_photo_merge::{Control, Error, Image, Mode, Options};
use std::sync::atomic::Ordering;

pub(crate) struct Job {
    pub control: Arc<Control>,
    pub processing: bool,
    pub error: Option<Error>,
}

impl Drop for Job {
    fn drop(&mut self) {
        self.control.cancelled.store(true, Ordering::Relaxed);
    }
}

pub(crate) fn mode_label(mode: Mode) -> &'static str {
    t(match mode {
        Mode::Align => "photo_merge.align",
        Mode::Focus => "photo_merge.focus",
        Mode::Hdr => "photo_merge.hdr",
        Mode::Panorama => "photo_merge.panorama",
    })
}

pub(crate) fn error_label(error: Error) -> &'static str {
    t(match error {
        Error::Invalid => "photo_merge.invalid",
        Error::TooLarge => "photo_merge.limit",
        Error::NoMatch => "photo_merge.no_match",
        Error::Cancelled => "common.cancel",
    })
}

impl Workspace {
    pub(crate) fn photo_merge_document(&self, id: DocumentId) -> Option<&Document> {
        self.doc.as_ref().filter(|doc| doc.id == id).or_else(|| {
            self.background_tabs
                .iter()
                .find(|tab| tab.doc.id == id)
                .map(|tab| &tab.doc)
        })
    }

    pub(crate) fn open_photo_merge(&mut self, cx: &mut Context<Self>) {
        let mut documents: Vec<_> = self.background_tabs.iter().map(|tab| tab.doc.id).collect();
        if let Some(doc) = &self.doc {
            documents.insert(self.active_tab.min(documents.len()), doc.id);
        }
        let included = (0..documents.len())
            .map(|i| i < schist_photo_merge::MAX_IMAGES)
            .collect();
        let options = Options {
            exposure_ev: vec![0.0; documents.len()],
            ..Default::default()
        };
        self.open_modal(
            Modal::PhotoMerge {
                documents,
                included,
                options,
            },
            cx,
        );
    }

    pub(crate) fn start_photo_merge(&mut self, cx: &mut Context<Self>) {
        if self
            .photo_merge_job
            .as_ref()
            .is_some_and(|job| job.processing)
        {
            return;
        }
        let Some(Modal::PhotoMerge {
            documents,
            included,
            options,
        }) = self.modal.clone()
        else {
            return;
        };
        let control = Arc::new(Control::default());
        let mut snapshots = Vec::new();
        let mut total = 0usize;
        let mut exposures = Vec::new();
        let prepared = (|| {
            let count = included.iter().filter(|&&yes| yes).count();
            if !(2..=schist_photo_merge::MAX_IMAGES).contains(&count) {
                return Err(Error::Invalid);
            }
            for (index, id) in documents.iter().enumerate() {
                if !included[index] {
                    continue;
                }
                let doc = self.photo_merge_document(*id).ok_or(Error::Invalid)?;
                total += schist_photo_merge::pixels(doc.width, doc.height)?;
                if total > schist_photo_merge::MAX_TOTAL_PIXELS {
                    return Err(Error::TooLarge);
                }
                // Layers share COW tiles; compositing and color conversion run
                // in the worker. Input tabs and their undo histories stay intact.
                let mut snapshot =
                    Document::new(doc.title.clone(), doc.width, doc.height, doc.depth);
                snapshot.tree = doc.tree.clone();
                snapshot.mode = doc.mode;
                snapshot.icc_profile = doc.icc_profile.clone();
                snapshot.resolution_dpi = doc.resolution_dpi;
                snapshots.push(snapshot);
                exposures.push(options.exposure_ev[index]);
            }
            Ok(())
        })();
        self.photo_merge_job = Some(Job {
            control: control.clone(),
            processing: prepared.is_ok(),
            error: prepared.err(),
        });
        cx.notify();
        if !self.photo_merge_job.as_ref().unwrap().processing {
            return;
        }
        let options = Options {
            exposure_ev: exposures,
            ..options
        };
        let title = mode_label(options.mode).to_string();
        let ticker = control.clone();
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(100))
                .await;
            if ticker.cancelled.load(Ordering::Relaxed) {
                break;
            }
            let keep = this
                .update(cx, |ws, cx| {
                    let keep = ws
                        .photo_merge_job
                        .as_ref()
                        .is_some_and(|job| job.processing && Arc::ptr_eq(&job.control, &ticker));
                    if keep {
                        cx.notify();
                    }
                    keep
                })
                .unwrap_or(false);
            if !keep {
                break;
            }
        })
        .detach();
        cx.spawn(async move |this, cx| {
            let worker = control.clone();
            let result = cx
                .background_executor()
                .spawn(async move { render_merge(snapshots, &options, &title, &worker) })
                .await;
            let _ = this.update(cx, |ws, cx| {
                if !ws
                    .photo_merge_job
                    .as_ref()
                    .is_some_and(|job| Arc::ptr_eq(&job.control, &control))
                {
                    return;
                }
                match result {
                    Ok(doc) if !control.cancelled.load(Ordering::Relaxed) => {
                        ws.close_modal(cx);
                        // Explicitly retain even pristine/untitled source tabs.
                        ws.open_in_tab(doc, false);
                        ws.status = t("common.done").into();
                    }
                    Ok(_) => ws.close_modal(cx),
                    Err(error) => {
                        if let Some(job) = ws.photo_merge_job.as_mut() {
                            job.processing = false;
                            job.error = Some(error);
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
}

fn render_merge(
    documents: Vec<Document>,
    options: &Options,
    title: &str,
    control: &Control,
) -> Result<Document, Error> {
    let mut images = Vec::new();
    for doc in &documents {
        control.check()?;
        // Composite in strips: cancellation stays responsive even while
        // flattening a large source, and the temporary buffer remains bounded.
        let mut rgba = Vec::new();
        rgba.try_reserve_exact(schist_photo_merge::pixels(doc.width, doc.height)? * 4)
            .map_err(|_| Error::TooLarge)?;
        let profile = if matches!(doc.mode, ColorMode::Cmyk | ColorMode::Lab) {
            schist_colormgmt::Profile::srgb()
        } else {
            doc.icc_profile
                .as_deref()
                .map(schist_colormgmt::Profile::from_bytes)
                .transpose()
                .map_err(|_| Error::Invalid)?
                .unwrap_or_else(schist_colormgmt::Profile::srgb)
        };
        let srgb = schist_colormgmt::Profile::srgb();
        let transform = if profile.icc_bytes() == srgb.icc_bytes() {
            schist_colormgmt::ColorTransform::identity()
        } else {
            schist_colormgmt::ColorTransform::new(
                &profile,
                &srgb,
                schist_colormgmt::Intent::RelativeColorimetric,
            )
            .map_err(|_| Error::Invalid)?
        };
        for y in (0..doc.height).step_by(64) {
            control.check()?;
            let rect = IntRect {
                left: 0,
                top: y as i32,
                right: doc.width as i32,
                bottom: (y + 64).min(doc.height) as i32,
            };
            let mut strip = schist_compositor::composite_region_f32_cpu(doc, rect);
            // The bracket workflow expects bounded developed exposures. Reject
            // invalid/extended input before the CMS can silently clip it.
            if strip
                .iter()
                .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
            {
                return Err(Error::Invalid);
            }
            transform.apply(&mut strip);
            rgba.extend_from_slice(&strip);
        }
        images.push(Image {
            width: doc.width,
            height: doc.height,
            rgba,
        });
    }
    let output = schist_photo_merge::merge(&images, options, control)?;
    let mut result = Document::new(title, output.width, output.height, Depth::ThirtyTwo);
    result.icc_profile = schist_colormgmt::Profile::srgb().icc_bytes().map(Vec::from);
    result.resolution_dpi = documents[0].resolution_dpi;
    if let Some(image) = output.merged {
        append_image(
            &mut result,
            title,
            &image,
            schist_photo_merge::Offset::default(),
            control,
        )?;
    } else {
        for ((image, source), offset) in images.iter().zip(&documents).zip(output.offsets) {
            append_image(&mut result, &source.title, image, offset, control)?;
        }
    }
    result.dirty = true;
    control.check()?;
    Ok(result)
}

fn append_image(
    doc: &mut Document,
    title: &str,
    image: &Image,
    offset: schist_photo_merge::Offset,
    control: &Control,
) -> Result<(), Error> {
    let mut layer = Layer::new_raster(title);
    // Crop samples as well as the canvas. Out-of-canvas source pixels otherwise
    // consume tiles and unexpectedly reappear after resizing the result.
    let x0 = 0.max(-offset.x) as u32;
    let x1 = image.width.min((doc.width as i32 - offset.x).max(0) as u32);
    let y0 = 0.max(-offset.y) as u32;
    let y1 = image
        .height
        .min((doc.height as i32 - offset.y).max(0) as u32);
    let mut strip = Vec::new();
    strip
        .try_reserve_exact((x1 - x0) as usize * 64 * 4)
        .map_err(|_| Error::TooLarge)?;
    for y in (y0..y1).step_by(64) {
        control.check()?;
        strip.clear();
        let bottom = (y + 64).min(y1);
        for row in y..bottom {
            let first = (row as usize * image.width as usize + x0 as usize) * 4;
            let last = (row as usize * image.width as usize + x1 as usize) * 4;
            strip.extend_from_slice(&image.rgba[first..last]);
        }
        let rect = IntRect {
            left: offset.x + x0 as i32,
            top: offset.y + y as i32,
            right: offset.x + x1 as i32,
            bottom: offset.y + bottom as i32,
        };
        blit_rgba_f32(
            &mut layer.as_raster_mut().unwrap().tiles,
            Depth::ThirtyTwo,
            rect,
            &strip,
        );
    }
    doc.push_layer(layer);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use schist_photo_merge::{linear_to_srgb, srgb_to_linear};

    fn exposed_document(ev: f32) -> Document {
        let mut doc = Document::new("Exposure", 5, 1, Depth::ThirtyTwo);
        let mut layer = Layer::new_raster("Input");
        let mut rgba = Vec::new();
        for radiance in [0.005f32, 0.08, 0.4, 2.0, 6.0] {
            let v = linear_to_srgb((radiance * ev.exp2()).min(1.0));
            rgba.extend_from_slice(&[v, v, v, 1.0]);
        }
        blit_rgba_f32(
            &mut layer.as_raster_mut().unwrap().tiles,
            doc.depth,
            doc.canvas_rect(),
            &rgba,
        );
        doc.push_layer(layer);
        doc
    }

    #[test]
    fn hdr_radiance_survives_document_and_psd_roundtrip() {
        let exposures = [-3.0, 0.0, 3.0];
        let sources = exposures.into_iter().map(exposed_document).collect();
        let options = Options {
            mode: Mode::Hdr,
            align: false,
            tone_map: false,
            exposure_ev: exposures.to_vec(),
            ..Default::default()
        };
        let result = render_merge(sources, &options, "HDR", &Control::default()).unwrap();
        assert_eq!(result.depth, Depth::ThirtyTwo);
        assert!(result.dirty);
        assert!(result.path.is_none());
        for psb in [false, true] {
            let bytes = schist_codec_psd::write_psd_with(&result, psb).unwrap();
            let reopened = schist_codec_psd::read_psd(&bytes).unwrap();
            assert_eq!(reopened.depth, Depth::ThirtyTwo);
            let samples =
                schist_compositor::composite_region_f32_cpu(&reopened, reopened.canvas_rect());
            for (i, radiance) in [0.005f32, 0.08, 0.4, 2.0, 6.0].iter().enumerate() {
                assert!(
                    (srgb_to_linear(samples[i * 4]) - radiance).abs() < 0.005,
                    "pixel {i}: {} expected {radiance}",
                    srgb_to_linear(samples[i * 4])
                );
            }
            assert!(samples[16] > 1.0);
        }
    }

    #[test]
    fn aligned_layers_crop_pixels_and_keep_source_names() {
        let mut result = Document::new("Aligned", 3, 1, Depth::ThirtyTwo);
        let image = Image {
            width: 5,
            height: 1,
            rgba: [0.7, 0.6, 0.5, 1.0].repeat(5),
        };
        append_image(
            &mut result,
            "Original A",
            &image,
            schist_photo_merge::Offset { x: -2, y: 0 },
            &Control::default(),
        )
        .unwrap();
        assert_eq!(result.tree.layers[0].name, "Original A");
        let outside = schist_compositor::composite_region_f32_cpu(
            &result,
            IntRect {
                left: -2,
                top: 0,
                right: 0,
                bottom: 1,
            },
        );
        assert!(outside.iter().all(|v| *v == 0.0));
        let samples = schist_compositor::composite_region_f32_cpu(&result, result.canvas_rect());
        assert_eq!(samples, [0.7, 0.6, 0.5, 1.0].repeat(3));
    }

    #[test]
    fn abandoned_job_cancels_without_publishing_a_document() {
        let control = Arc::new(Control::default());
        let job = Job {
            control: control.clone(),
            processing: true,
            error: None,
        };
        drop(job);
        assert!(control.cancelled.load(Ordering::Relaxed));
        let result = render_merge(
            vec![exposed_document(0.0), exposed_document(1.0)],
            &Options::default(),
            "Cancelled",
            &control,
        );
        assert_eq!(result.unwrap_err(), Error::Cancelled);
    }
}
