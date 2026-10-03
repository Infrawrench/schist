//! Camera Raw local adjustments: the masks' editing session, their
//! on-canvas handles and brush, and the stage of the development that
//! applies them.
//!
//! The masks live in the dialog's session until OK, when they are stored
//! with the development. Rendering is in two halves so a mask edit does
//! not decode the sensor data again: [`finish_raw`] runs the masks over a
//! globally developed picture, and the preview keeps that picture between
//! edits that leave the global sliders alone.

use super::*;
use schist_core::raw_masks::LOCAL_CONTROLS;
use schist_core::{
    BrushStroke, DetectedKind, LocalAdjustments, LocalMask, MaskCombine, MaskComponent, MaskRaster,
    MaskShape,
};
use schist_i18n::{t, tf};
use schist_plugin_api::FilterValues;

/// Detections run no larger than this on the longer side. The networks
/// see a few hundred pixels whatever they are given; this is for the edge
/// refinement, which is what decides how well a mask hugs its subject.
const DETECT_SIDE: usize = 1024;
/// The sky search is a flood fill over colour and texture, which wants
/// fewer pixels still.
const SKY_SIDE: usize = 512;
/// The overlay colour, the red Lightroom uses, at half strength.
const OVERLAY: [f32; 3] = [1.0, 0.18, 0.18];

/// The brush settings shared by every stroke painted in a session.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct BrushSettings {
    /// Radius as a percentage of the image's longer side.
    pub size: f32,
    pub feather: f32,
    pub flow: f32,
    pub erase: bool,
}

impl Default for BrushSettings {
    fn default() -> Self {
        BrushSettings {
            size: 4.0,
            feather: 50.0,
            flow: 100.0,
            erase: false,
        }
    }
}

/// What a pointer drag on the canvas is doing.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Drag {
    /// Painting the stroke held in `RawMaskEditor::stroke`.
    Paint,
    /// Moving one of a gradient's handles, keeping the grabbed offset.
    Handle(Handle, (f32, f32)),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Handle {
    LinearStart,
    LinearEnd,
    LinearMiddle,
    RadialCenter,
    RadialMajor,
    RadialMinor,
}

/// The Camera Raw dialog's masks while it is open.
pub(crate) struct RawMaskEditor {
    pub layer: schist_core::LayerId,
    pub masks: Vec<LocalMask>,
    pub selected: Option<usize>,
    /// The component being edited within the selected mask.
    pub component: usize,
    pub overlay: bool,
    pub brush: BrushSettings,
    /// A detection is running; the dialog says so.
    pub detecting: bool,
    detect_seq: u64,
    stroke: Option<BrushStroke>,
    drag: Option<Drag>,
}

impl RawMaskEditor {
    fn new(layer: schist_core::LayerId, masks: Vec<LocalMask>) -> Self {
        RawMaskEditor {
            layer,
            selected: (!masks.is_empty()).then_some(0),
            masks,
            component: 0,
            overlay: false,
            brush: BrushSettings::default(),
            detecting: false,
            detect_seq: 0,
            stroke: None,
            drag: None,
        }
    }

    pub fn mask(&self) -> Option<&LocalMask> {
        self.masks.get(self.selected?)
    }

    fn mask_mut(&mut self) -> Option<&mut LocalMask> {
        self.masks.get_mut(self.selected?)
    }

    pub fn shape(&self) -> Option<&MaskShape> {
        Some(&self.mask()?.components.get(self.component)?.shape)
    }

    fn shape_mut(&mut self) -> Option<&mut MaskShape> {
        let component = self.component;
        Some(&mut self.mask_mut()?.components.get_mut(component)?.shape)
    }

    /// Whether the canvas belongs to the mask: a brush to paint with or a
    /// gradient to drag.
    fn edits_canvas(&self) -> bool {
        matches!(
            self.shape(),
            Some(MaskShape::Brush { .. } | MaskShape::Linear { .. } | MaskShape::Radial { .. })
        )
    }
}

/// Which kind of component a dialog button adds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NewShape {
    Brush,
    Linear,
    Radial,
    Detected(DetectedKind),
}

impl NewShape {
    pub const ALL: [NewShape; 6] = [
        NewShape::Brush,
        NewShape::Linear,
        NewShape::Radial,
        NewShape::Detected(DetectedKind::Subject),
        NewShape::Detected(DetectedKind::Sky),
        NewShape::Detected(DetectedKind::Background),
    ];

    pub fn label(self) -> &'static str {
        t(match self {
            NewShape::Brush => "tool.brush.name",
            NewShape::Linear => "dialog.raw_masks.linear",
            NewShape::Radial => "tool.gradient.radial.name",
            NewShape::Detected(DetectedKind::Subject) => "dialog.raw_masks.subject",
            NewShape::Detected(DetectedKind::Sky) => "dialog.raw_masks.sky",
            NewShape::Detected(DetectedKind::Background) => "dialog.raw_masks.background",
        })
    }

    pub fn of(shape: &MaskShape) -> NewShape {
        match shape {
            MaskShape::Brush { .. } => NewShape::Brush,
            MaskShape::Linear { .. } => NewShape::Linear,
            MaskShape::Radial { .. } => NewShape::Radial,
            MaskShape::Detected { kind, .. } => NewShape::Detected(*kind),
        }
    }

    fn shape(self) -> MaskShape {
        match self {
            NewShape::Brush => MaskShape::Brush {
                strokes: Vec::new(),
            },
            NewShape::Linear => MaskShape::default_linear(),
            NewShape::Radial => MaskShape::default_radial(),
            NewShape::Detected(kind) => MaskShape::Detected { kind, raster: None },
        }
    }
}

/// A mask's controls as Camera Raw filter values.
pub(crate) fn local_values(adjustments: &LocalAdjustments) -> FilterValues {
    let mut values = FilterValues::default();
    for (key, ..) in LOCAL_CONTROLS {
        values.set(key, adjustments.get(key));
    }
    values
}

/// Run every active mask over a developed picture, in order. Detected
/// components are filled in first when `detect` is set, from the picture
/// as the global controls left it, and the filled-in masks are what the
/// caller should store.
pub(crate) fn finish_raw(
    rgba: &mut [f32],
    width: usize,
    height: usize,
    masks: &mut [LocalMask],
    detect: bool,
) -> anyhow::Result<()> {
    if detect {
        detect_missing(masks, rgba, width, height)?;
    }
    for mask in masks.iter().filter(|m| m.is_active()) {
        let coverage = mask.coverage(width, height);
        schist_filters_core::camera_raw::apply_local(
            rgba,
            width,
            height,
            &local_values(&mask.adjustments),
            &coverage,
        );
    }
    Ok(())
}

/// Run the detector for every detected component that has no raster.
/// Each kind runs at most once however many components ask for it.
pub(crate) fn detect_missing(
    masks: &mut [LocalMask],
    rgba: &[f32],
    width: usize,
    height: usize,
) -> anyhow::Result<()> {
    let mut found: Vec<(DetectedKind, Arc<MaskRaster>)> = Vec::new();
    for mask in masks.iter_mut() {
        for component in &mut mask.components {
            let MaskShape::Detected { kind, raster } = &mut component.shape else {
                continue;
            };
            if raster.is_some() {
                continue;
            }
            let detected = match found.iter().find(|(k, _)| k == kind) {
                Some((_, r)) => r.clone(),
                None => {
                    let r = Arc::new(detect(*kind, rgba, width, height)?);
                    found.push((*kind, r.clone()));
                    r
                }
            };
            *raster = Some(detected);
        }
    }
    Ok(())
}

/// The reason a detection could not run because a model is missing, in
/// the same words the Neural Filters use.
fn missing_model(id: &str) -> anyhow::Error {
    let name = schist_neural::spec(id).map(|s| s.name).unwrap_or(id);
    anyhow::anyhow!(
        "{} {}",
        tf!("workspace.raw_masks.no_model", model = name),
        t("filter.neural.msg.get_model")
    )
}

/// Find a subject, the sky or the background in a straight-alpha RGBA
/// picture, as a coverage raster.
pub(crate) fn detect(
    kind: DetectedKind,
    rgba: &[f32],
    width: usize,
    height: usize,
) -> anyhow::Result<MaskRaster> {
    anyhow::ensure!(
        width > 0 && height > 0 && rgba.len() == width * height * 4,
        "image is {width}x{height} but has {} floats",
        rgba.len()
    );
    let (coverage, w, h) = match kind {
        DetectedKind::Subject | DetectedKind::Background => {
            let (rgb, w, h) = downsample(rgba, width, height, DETECT_SIDE);
            let mut subject = subject(&rgb, w, h)?;
            if kind == DetectedKind::Background {
                subject.iter_mut().for_each(|v| *v = 1.0 - *v);
            }
            (subject, w, h)
        }
        DetectedKind::Sky => {
            let (rgb, w, h) = downsample(rgba, width, height, SKY_SIDE);
            let depth = schist_neural::get("depth").and_then(|model| {
                schist_neural::depth_map(&model, &rgb, w, h)
                    .map_err(|e| log::warn!("sky depth: {e:#}"))
                    .ok()
            });
            (sky(&rgb, w, h, depth.as_deref()), w, h)
        }
    };
    MaskRaster::from_coverage(w as u32, h as u32, &coverage)
        .ok_or_else(|| anyhow::anyhow!("detected mask is {w}x{h}"))
}

/// Interleaved RGB no larger than `side` on its longer side, by box
/// averaging.
fn downsample(rgba: &[f32], width: usize, height: usize, side: usize) -> (Vec<f32>, usize, usize) {
    let scale = (width.max(height) as f32 / side as f32).max(1.0);
    let w = ((width as f32 / scale).round() as usize).clamp(1, width);
    let h = ((height as f32 / scale).round() as usize).clamp(1, height);
    let mut out = vec![0.0f32; w * h * 3];
    for y in 0..h {
        let (y0, y1) = (
            y * height / h,
            ((y + 1) * height / h).max(y * height / h + 1),
        );
        for x in 0..w {
            let (x0, x1) = (x * width / w, ((x + 1) * width / w).max(x * width / w + 1));
            let mut sum = [0.0f32; 3];
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let i = (sy * width + sx) * 4;
                    for c in 0..3 {
                        sum[c] += rgba[i + c];
                    }
                }
            }
            let n = ((y1 - y0) * (x1 - x0)) as f32;
            for c in 0..3 {
                out[(y * w + x) * 3 + c] = sum[c] / n;
            }
        }
    }
    (out, w, h)
}

/// Settle a rough mask's edges against the picture with Refine Mask's
/// colour-guided estimate, then soften them by a pixel.
fn refine(rgb: &[f32], w: usize, h: usize, mask: Vec<f32>, radius: f32) -> Vec<f32> {
    let mut image = schist_core::mask_refine::Image {
        width: w,
        height: h,
        pixels: rgb
            .as_chunks::<3>()
            .0
            .iter()
            .map(|p| Rgba {
                r: p[0],
                g: p[1],
                b: p[2],
                a: 1.0,
            })
            .collect(),
        mask,
        scale: 1.0,
    };
    image.refine(schist_core::mask_refine::Settings {
        radius,
        refine: 1.0,
        smooth: 0.0,
        feather: 1.0,
        shift: 0.0,
        decontaminate: 0.0,
    });
    image.mask
}

/// The salient subject, through the Object Selection model.
fn subject(rgb: &[f32], w: usize, h: usize) -> anyhow::Result<Vec<f32>> {
    let model = schist_neural::get("segment").ok_or_else(|| missing_model("segment"))?;
    let map = schist_neural::segment(&model, rgb, w, h)?;
    // As Object Selection does: cut at half of what the network was
    // willing to commit to, so a hesitant answer is still cut somewhere
    // down its own slope. The ramp either side of the cut is left soft
    // for the refinement to settle.
    let peak = map.iter().copied().fold(0.0f32, f32::max);
    if peak < 0.2 {
        return Ok(vec![0.0; w * h]);
    }
    let cut = (peak * 0.5).clamp(0.2, 0.5);
    let rough = map
        .iter()
        .map(|v| ((v - cut) / 0.2 + 0.5).clamp(0.0, 1.0))
        .collect();
    Ok(refine(rgb, w, h, rough, 4.0))
}

/// Sky, found without a sky model: bright, smooth, blue-or-neutral
/// pixels connected to the top edge, and far away when a depth map is to
/// hand.
fn sky(rgb: &[f32], w: usize, h: usize, depth: Option<&[f32]>) -> Vec<f32> {
    let luma = |i: usize| 0.299 * rgb[i * 3] + 0.587 * rgb[i * 3 + 1] + 0.114 * rgb[i * 3 + 2];
    // Texture: the largest step to a neighbour. Sky is smooth; foliage,
    // buildings and terrain are not.
    let mut texture = vec![0.0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let mut step = 0.0f32;
            if x + 1 < w {
                step = step.max((luma(i) - luma(i + 1)).abs());
            }
            if y + 1 < h {
                step = step.max((luma(i) - luma(i + w)).abs());
            }
            texture[i] = step;
        }
    }
    let smooth = box_max(&texture, w, h, 1);
    let likely: Vec<bool> = (0..w * h)
        .map(|i| {
            let (r, g, b) = (rgb[i * 3], rgb[i * 3 + 1], rgb[i * 3 + 2]);
            let l = luma(i);
            let spread = r.max(g).max(b) - r.min(g).min(b);
            let blue = b >= r && b + 0.02 >= g * 0.9 && l > 0.25;
            let overcast = spread < 0.12 && l > 0.55;
            let far = depth.is_none_or(|d| d[i] < 0.4);
            (blue || overcast) && smooth[i] < 0.06 && far
        })
        .collect();
    // Flood from the top row through likely pixels whose colour changes
    // gently, so a blue car or a white wall lower down is not sky.
    let mut sky = vec![false; w * h];
    let mut stack: Vec<usize> = (0..w).filter(|&x| likely[x]).collect();
    for &i in &stack {
        sky[i] = true;
    }
    let close = |a: usize, b: usize| (0..3).all(|c| (rgb[a * 3 + c] - rgb[b * 3 + c]).abs() < 0.06);
    while let Some(i) = stack.pop() {
        let (x, y) = (i % w, i / w);
        for n in [
            (x > 0).then(|| i - 1),
            (x + 1 < w).then(|| i + 1),
            (y > 0).then(|| i - w),
            (y + 1 < h).then(|| i + w),
        ]
        .into_iter()
        .flatten()
        {
            if !sky[n] && likely[n] && close(i, n) {
                sky[n] = true;
                stack.push(n);
            }
        }
    }
    // A few stray pixels are not a sky.
    if sky.iter().filter(|s| **s).count() * 200 < w * h {
        return vec![0.0; w * h];
    }
    let rough = sky.iter().map(|s| if *s { 1.0 } else { 0.0 }).collect();
    refine(rgb, w, h, rough, 6.0)
}

/// The largest value within `radius` of each sample.
fn box_max(values: &[f32], w: usize, h: usize, radius: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut m = 0.0f32;
            for sy in y.saturating_sub(radius)..(y + radius + 1).min(h) {
                for sx in x.saturating_sub(radius)..(x + radius + 1).min(w) {
                    m = m.max(values[sy * w + sx]);
                }
            }
            out[y * w + x] = m;
        }
    }
    out
}

/// Tint `rgba` by a mask's coverage, as the overlay shows it.
pub(crate) fn tint(rgba: &mut [f32], coverage: &[f32]) {
    for (p, c) in rgba.as_chunks_mut::<4>().0.iter_mut().zip(coverage) {
        let k = 0.5 * c.clamp(0.0, 1.0);
        for i in 0..3 {
            p[i] += (OVERLAY[i] - p[i]) * k;
        }
    }
}

impl Workspace {
    /// Start a mask session for the active RAW layer's development.
    pub(super) fn begin_raw_mask_editing(&mut self) {
        let Some((layer, masks)) = self.doc.as_ref().and_then(|doc| {
            let id = doc.active_layer?;
            let raw = doc.tree.find(id)?.raw.as_deref()?;
            Some((id, raw.masks.clone()))
        }) else {
            self.raw_masks = None;
            return;
        };
        self.raw_masks = Some(RawMaskEditor::new(layer, masks));
        self.raw_preview_base = None;
    }

    /// End the session and hand back its masks, for OK.
    pub(crate) fn take_raw_mask_edits(&mut self) -> Option<Vec<LocalMask>> {
        self.raw_preview_base = None;
        self.raw_masks.take().map(|e| e.masks)
    }

    pub(crate) fn end_raw_mask_editing(&mut self) {
        self.raw_masks = None;
        self.raw_preview_base = None;
    }

    /// Re-render the preview after a mask changed, if Preview is on.
    pub(crate) fn refresh_raw_masks(&mut self, cx: &mut Context<Self>) {
        let next = match &self.modal {
            Some(Modal::Filter {
                id,
                values,
                preview: true,
                ..
            }) => Some((*id, values.clone())),
            _ => None,
        };
        if let Some((id, values)) = next {
            self.preview_filter(id, Some(&values), cx);
        }
        cx.notify();
    }

    /// Masks and the coverage to tint, for a preview render.
    pub(super) fn raw_mask_preview_inputs(&self) -> (Vec<LocalMask>, Option<usize>) {
        match &self.raw_masks {
            Some(editor) => (
                editor.masks.clone(),
                editor.selected.filter(|_| editor.overlay),
            ),
            None => (Vec::new(), None),
        }
    }

    /// Add a new mask, or a component to the selected one.
    pub(crate) fn add_raw_mask(
        &mut self,
        shape: NewShape,
        to_selected: bool,
        cx: &mut Context<Self>,
    ) {
        if let NewShape::Detected(DetectedKind::Subject | DetectedKind::Background) = shape {
            if !schist_neural::installed("segment") {
                self.status = missing_model("segment").to_string().into();
                cx.notify();
                return;
            }
        }
        let Some(editor) = self.raw_masks.as_mut() else {
            return;
        };
        match editor.selected.filter(|_| to_selected) {
            Some(index)
                if editor.masks[index].components.len()
                    < schist_core::raw_masks::MAX_COMPONENTS =>
            {
                let mask = &mut editor.masks[index];
                mask.components.push(MaskComponent::new(shape.shape()));
                editor.component = mask.components.len() - 1;
            }
            Some(_) => return,
            None => {
                if editor.masks.len() >= schist_core::raw_masks::MAX_MASKS {
                    return;
                }
                editor.masks.push(LocalMask::new(shape.shape()));
                editor.selected = Some(editor.masks.len() - 1);
                editor.component = 0;
            }
        }
        if let NewShape::Detected(_) = shape {
            self.run_raw_mask_detection(cx);
        }
        self.refresh_raw_masks(cx);
    }

    /// Detect every component still waiting for it, in the background,
    /// from the picture the preview last developed.
    fn run_raw_mask_detection(&mut self, cx: &mut Context<Self>) {
        let source = match (&self.raw_preview_base, &self.filter_preview) {
            (Some(base), _) => Some((base.width, base.height, base.rgba.clone())),
            (None, Some(preview)) => Some((
                preview.region.width() as usize,
                preview.region.height() as usize,
                Arc::new(preview.original.clone()),
            )),
            _ => None,
        };
        let Some((width, height, rgba)) = source else {
            return;
        };
        let Some(editor) = self.raw_masks.as_mut() else {
            return;
        };
        editor.detect_seq = editor.detect_seq.wrapping_add(1);
        editor.detecting = true;
        let sequence = editor.detect_seq;
        let mut masks = editor.masks.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(
                    async move { detect_missing(&mut masks, &rgba, width, height).map(|_| masks) },
                )
                .await;
            this.update(cx, |ws, cx| {
                let Some(editor) = ws.raw_masks.as_mut() else {
                    return;
                };
                if editor.detect_seq != sequence {
                    return;
                }
                editor.detecting = false;
                let detected = match result {
                    Ok(detected) => detected,
                    Err(err) => {
                        ws.status = tf!("workspace.raw_masks.detect_failed", error = err).into();
                        cx.notify();
                        return;
                    }
                };
                // The masks may have been edited meanwhile; fill in only the
                // components that are still the ones that were detected.
                let mut empty_sky = false;
                for (mask, done) in editor.masks.iter_mut().zip(&detected) {
                    for (component, finished) in mask.components.iter_mut().zip(&done.components) {
                        if let (
                            MaskShape::Detected {
                                kind,
                                raster: slot @ None,
                            },
                            MaskShape::Detected {
                                kind: found,
                                raster: Some(raster),
                            },
                        ) = (&mut component.shape, &finished.shape)
                        {
                            if kind == found {
                                empty_sky |= *kind == DetectedKind::Sky
                                    && raster.data.iter().all(|v| *v == 0);
                                *slot = Some(raster.clone());
                            }
                        }
                    }
                }
                if empty_sky {
                    ws.status = t("workspace.raw_masks.no_sky").into();
                }
                ws.refresh_raw_masks(cx);
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn select_raw_mask(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(editor) = self.raw_masks.as_mut() {
            if index < editor.masks.len() {
                editor.selected = Some(index);
                editor.component = 0;
            }
        }
        self.refresh_raw_masks(cx);
    }

    pub(crate) fn select_raw_mask_component(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(editor) = self.raw_masks.as_mut() {
            if editor.mask().is_some_and(|m| index < m.components.len()) {
                editor.component = index;
            }
        }
        cx.notify();
    }

    pub(crate) fn delete_raw_mask(&mut self, cx: &mut Context<Self>) {
        if let Some(editor) = self.raw_masks.as_mut() {
            if let Some(index) = editor.selected {
                editor.masks.remove(index);
                editor.selected =
                    (!editor.masks.is_empty()).then(|| index.min(editor.masks.len() - 1));
                editor.component = 0;
            }
        }
        self.refresh_raw_masks(cx);
    }

    pub(crate) fn remove_raw_mask_component(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(editor) = self.raw_masks.as_mut() else {
            return;
        };
        let Some(mask) = editor.mask_mut() else {
            return;
        };
        if index >= mask.components.len() {
            return;
        }
        mask.components.remove(index);
        if mask.components.is_empty() {
            self.delete_raw_mask(cx);
            return;
        }
        editor.component = editor.component.min(
            editor
                .mask()
                .map(|m| m.components.len().saturating_sub(1))
                .unwrap_or(0),
        );
        self.refresh_raw_masks(cx);
    }

    /// Change the selected mask, then refresh the preview.
    pub(crate) fn edit_raw_mask(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut LocalMask)) {
        if let Some(mask) = self.raw_masks.as_mut().and_then(|e| e.mask_mut()) {
            f(mask);
        }
        self.refresh_raw_masks(cx);
    }

    /// Change one component of the selected mask, then refresh the preview.
    pub(crate) fn edit_raw_mask_component(
        &mut self,
        index: usize,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut MaskComponent),
    ) {
        if let Some(component) = self
            .raw_masks
            .as_mut()
            .and_then(|e| e.mask_mut())
            .and_then(|m| m.components.get_mut(index))
        {
            f(component);
        }
        self.refresh_raw_masks(cx);
    }

    pub(crate) fn set_raw_mask_combine(
        &mut self,
        index: usize,
        combine: MaskCombine,
        cx: &mut Context<Self>,
    ) {
        self.edit_raw_mask_component(index, cx, |c| c.combine = combine);
    }

    pub(crate) fn set_raw_mask_brush(&mut self, f: impl FnOnce(&mut BrushSettings)) {
        if let Some(editor) = self.raw_masks.as_mut() {
            f(&mut editor.brush);
        }
    }

    pub(crate) fn toggle_raw_mask_overlay(&mut self, cx: &mut Context<Self>) {
        if let Some(editor) = self.raw_masks.as_mut() {
            editor.overlay = !editor.overlay;
        }
        self.refresh_raw_masks(cx);
    }

    // ----- the canvas -----

    /// The developed picture's size: masks are drawn in fractions of it.
    fn raw_mask_size(&self) -> Option<(f32, f32)> {
        let preview = self.filter_preview.as_ref()?;
        let editor = self.raw_masks.as_ref()?;
        (preview.layer == editor.layer && preview.whole_layer).then(|| {
            (
                preview.region.width() as f32,
                preview.region.height() as f32,
            )
        })
    }

    /// Whether a press on the canvas paints or drags a mask.
    pub(crate) fn raw_mask_canvas_active(&self) -> bool {
        self.raw_mask_size().is_some()
            && self.raw_masks.as_ref().is_some_and(|e| e.edits_canvas())
            && matches!(&self.modal, Some(Modal::Filter { .. }))
    }

    pub(crate) fn raw_mask_dragging(&self) -> bool {
        self.raw_masks.as_ref().is_some_and(|e| e.drag.is_some())
    }

    /// Document point to mask coordinates, and back.
    fn doc_to_mask(&self, p: (f32, f32)) -> Option<[f32; 2]> {
        let (w, h) = self.raw_mask_size()?;
        let origin = self.filter_preview.as_ref()?.region;
        Some([
            (p.0 - origin.left as f32) / w,
            (p.1 - origin.top as f32) / h,
        ])
    }

    fn mask_to_doc(&self, p: [f32; 2]) -> Option<(f32, f32)> {
        let (w, h) = self.raw_mask_size()?;
        let origin = self.filter_preview.as_ref()?.region;
        Some((p[0] * w + origin.left as f32, p[1] * h + origin.top as f32))
    }

    /// A gradient's handles in document coordinates.
    fn raw_mask_handles(&self) -> Vec<(Handle, (f32, f32))> {
        let Some((w, h)) = self.raw_mask_size() else {
            return Vec::new();
        };
        let long = w.max(h);
        let to_doc = |p: [f32; 2]| self.mask_to_doc(p);
        let mut out = Vec::new();
        match self.raw_masks.as_ref().and_then(|e| e.shape()) {
            Some(MaskShape::Linear { start, end }) => {
                let middle = [(start[0] + end[0]) / 2.0, (start[1] + end[1]) / 2.0];
                for (handle, p) in [
                    (Handle::LinearStart, *start),
                    (Handle::LinearEnd, *end),
                    (Handle::LinearMiddle, middle),
                ] {
                    if let Some(p) = to_doc(p) {
                        out.push((handle, p));
                    }
                }
            }
            Some(MaskShape::Radial {
                center,
                radius,
                angle,
                ..
            }) => {
                let Some(c) = to_doc(*center) else {
                    return out;
                };
                let (sin, cos) = angle.to_radians().sin_cos();
                out.push((Handle::RadialCenter, c));
                out.push((
                    Handle::RadialMajor,
                    (c.0 + radius[0] * long * cos, c.1 + radius[0] * long * sin),
                ));
                out.push((
                    Handle::RadialMinor,
                    (c.0 - radius[1] * long * sin, c.1 + radius[1] * long * cos),
                ));
            }
            _ => {}
        }
        out
    }

    /// A press on the canvas: grab a handle, start a stroke, or start
    /// drawing a new gradient where there is no handle.
    pub(crate) fn raw_mask_down(&mut self, p: (f32, f32), cx: &mut Context<Self>) {
        let target = 11.0 / self.zoom;
        let grabbed = self
            .raw_mask_handles()
            .into_iter()
            .map(|(handle, h)| (handle, h, (h.0 - p.0).hypot(h.1 - p.1)))
            .filter(|(.., d)| *d <= target)
            .min_by(|a, b| a.2.total_cmp(&b.2));
        let Some(at) = self.doc_to_mask(p) else {
            return;
        };
        let Some((w, h)) = self.raw_mask_size() else {
            return;
        };
        let Some(editor) = self.raw_masks.as_mut() else {
            return;
        };
        if let Some((handle, h, _)) = grabbed {
            editor.drag = Some(Drag::Handle(handle, (h.0 - p.0, h.1 - p.1)));
            cx.notify();
            return;
        }
        let brush = editor.brush;
        match editor.shape_mut() {
            Some(MaskShape::Brush { .. }) => {
                editor.stroke = Some(BrushStroke {
                    erase: brush.erase,
                    size: brush.size / 100.0,
                    feather: brush.feather,
                    flow: brush.flow,
                    points: vec![at],
                });
                editor.drag = Some(Drag::Paint);
            }
            Some(MaskShape::Linear { start, end }) => {
                *start = at;
                *end = at;
                editor.drag = Some(Drag::Handle(Handle::LinearEnd, (0.0, 0.0)));
            }
            Some(MaskShape::Radial {
                center,
                radius,
                angle,
                ..
            }) => {
                *center = at;
                // A click without a drag leaves a small circle rather than
                // nothing at all.
                let small = 2.0 / w.max(h);
                *radius = [small, small];
                *angle = 0.0;
                editor.drag = Some(Drag::Handle(Handle::RadialMajor, (0.0, 0.0)));
            }
            _ => {}
        }
        cx.notify();
    }

    pub(crate) fn raw_mask_move(&mut self, p: (f32, f32), cx: &mut Context<Self>) {
        let Some(drag) = self.raw_masks.as_ref().and_then(|e| e.drag) else {
            return;
        };
        match drag {
            Drag::Paint => {
                let Some(at) = self.doc_to_mask(p) else {
                    return;
                };
                let Some((w, h)) = self.raw_mask_size() else {
                    return;
                };
                let Some(stroke) = self.raw_masks.as_mut().and_then(|e| e.stroke.as_mut()) else {
                    return;
                };
                // Thin out the points: a dab every quarter radius is
                // all the rasteriser lays down anyway.
                let last = *stroke.points.last().unwrap_or(&at);
                let step = (stroke.size * w.max(h) * 0.2).max(1.0);
                if ((at[0] - last[0]) * w).hypot((at[1] - last[1]) * h) >= step
                    && stroke.points.len() < schist_core::raw_masks::MAX_STROKE_POINTS
                {
                    stroke.points.push(at);
                }
                cx.notify();
            }
            Drag::Handle(handle, offset) => {
                self.move_raw_mask_handle(handle, (p.0 + offset.0, p.1 + offset.1));
                self.refresh_raw_masks(cx);
            }
        }
    }

    pub(crate) fn raw_mask_up(&mut self, p: (f32, f32), cx: &mut Context<Self>) {
        self.raw_mask_move(p, cx);
        let Some(editor) = self.raw_masks.as_mut() else {
            return;
        };
        let drag = editor.drag.take();
        if drag == Some(Drag::Paint) {
            let stroke = editor.stroke.take();
            if let (Some(stroke), Some(MaskShape::Brush { strokes })) = (stroke, editor.shape_mut())
            {
                if strokes.len() < schist_core::raw_masks::MAX_STROKES {
                    strokes.push(stroke);
                }
            }
            self.refresh_raw_masks(cx);
        }
        cx.notify();
    }

    fn move_raw_mask_handle(&mut self, handle: Handle, p: (f32, f32)) {
        let Some(at) = self.doc_to_mask(p) else {
            return;
        };
        let Some((w, h)) = self.raw_mask_size() else {
            return;
        };
        let long = w.max(h);
        let Some(shape) = self.raw_masks.as_mut().and_then(|e| e.shape_mut()) else {
            return;
        };
        match (shape, handle) {
            (MaskShape::Linear { start, .. }, Handle::LinearStart) => *start = at,
            (MaskShape::Linear { end, .. }, Handle::LinearEnd) => *end = at,
            (MaskShape::Linear { start, end }, Handle::LinearMiddle) => {
                let middle = [(start[0] + end[0]) / 2.0, (start[1] + end[1]) / 2.0];
                let delta = [at[0] - middle[0], at[1] - middle[1]];
                for p in [start, end] {
                    p[0] += delta[0];
                    p[1] += delta[1];
                }
            }
            (MaskShape::Radial { center, .. }, Handle::RadialCenter) => *center = at,
            (
                MaskShape::Radial {
                    center,
                    radius,
                    angle,
                    ..
                },
                Handle::RadialMajor,
            ) => {
                // The major handle sets the radius and the rotation at once.
                let (dx, dy) = ((at[0] - center[0]) * w, (at[1] - center[1]) * h);
                radius[0] = (dx.hypot(dy) / long).max(0.001);
                if dx != 0.0 || dy != 0.0 {
                    *angle = dy.atan2(dx).to_degrees().rem_euclid(360.0);
                }
            }
            (
                MaskShape::Radial {
                    center,
                    radius,
                    angle,
                    ..
                },
                Handle::RadialMinor,
            ) => {
                // Only the distance along the minor axis counts.
                let (sin, cos) = angle.to_radians().sin_cos();
                let (dx, dy) = ((at[0] - center[0]) * w, (at[1] - center[1]) * h);
                radius[1] = ((-dx * sin + dy * cos).abs() / long).max(0.001);
            }
            _ => {}
        }
    }

    /// Guides, handles and the stroke in progress, over the canvas.
    pub(super) fn paint_raw_masks(
        &self,
        job: &mut PaintJob,
        screen: &impl Fn(f32, f32) -> Point<Pixels>,
    ) {
        let Some((w, h)) = self.raw_mask_size() else {
            return;
        };
        let Some(editor) = self.raw_masks.as_ref() else {
            return;
        };
        let colour: gpui::Hsla = gpui::rgb(0x44AAFF).into();
        let long = w.max(h);
        let doc = |p: [f32; 2]| self.mask_to_doc(p);
        let ellipse = |c: (f32, f32), rx: f32, ry: f32, angle: f32| -> Vec<Point<Pixels>> {
            let (sin, cos) = angle.to_radians().sin_cos();
            (0..=64)
                .map(|i| {
                    let a = i as f32 / 64.0 * std::f32::consts::TAU;
                    let (u, v) = (rx * a.cos(), ry * a.sin());
                    screen(c.0 + u * cos - v * sin, c.1 + u * sin + v * cos)
                })
                .collect()
        };
        match editor.shape() {
            Some(MaskShape::Linear { start, end }) => {
                let (Some(s), Some(e)) = (doc(*start), doc(*end)) else {
                    return;
                };
                let (dx, dy) = (e.0 - s.0, e.1 - s.1);
                let len = dx.hypot(dy).max(1e-3);
                // Each edge of the gradient is a line across the picture,
                // square to the direction it runs in.
                let across = (-dy / len * w.hypot(h), dx / len * w.hypot(h));
                for p in [s, e] {
                    job.polylines.push((
                        vec![
                            screen(p.0 - across.0, p.1 - across.1),
                            screen(p.0 + across.0, p.1 + across.1),
                        ],
                        colour,
                    ));
                }
                job.polylines
                    .push((vec![screen(s.0, s.1), screen(e.0, e.1)], colour));
            }
            Some(MaskShape::Radial {
                center,
                radius,
                angle,
                feather,
            }) => {
                let Some(c) = doc(*center) else {
                    return;
                };
                let (rx, ry) = (radius[0] * long, radius[1] * long);
                job.polylines.push((ellipse(c, rx, ry, *angle), colour));
                let inner = 1.0 - feather / 100.0;
                if inner > 0.0 && inner < 1.0 {
                    job.polylines
                        .push((ellipse(c, rx * inner, ry * inner, *angle), colour));
                }
            }
            _ => {}
        }
        for (_, p) in self.raw_mask_handles() {
            let p = screen(p.0, p.1);
            let r = px(5.0);
            job.markers.push(Marker {
                bounds: Bounds {
                    origin: point(p.x - r, p.y - r),
                    size: size(r * 2.0, r * 2.0),
                },
                fill: colour,
                selected: false,
            });
        }
        if let Some(stroke) = &editor.stroke {
            let points: Vec<_> = stroke
                .points
                .iter()
                .filter_map(|p| doc(*p))
                .map(|p| screen(p.0, p.1))
                .collect();
            if points.len() > 1 {
                job.polylines.push((points, colour));
            }
            if let Some(last) = stroke.points.last().and_then(|p| doc(*p)) {
                let r = stroke.size * long;
                job.polylines.push((ellipse(last, r, r, 0.0), colour));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A picture with a blue sky over a textured, warm ground.
    fn landscape(w: usize, h: usize) -> Vec<f32> {
        (0..w * h)
            .flat_map(|i| {
                let (x, y) = (i % w, i / w);
                if y < h / 2 {
                    let shade = 0.02 * y as f32 / h as f32;
                    [0.35 + shade, 0.55 + shade, 0.9, 1.0]
                } else {
                    let noise = ((x * 7 + y * 13) % 5) as f32 * 0.06;
                    [0.4 + noise, 0.3 + noise, 0.15, 1.0]
                }
            })
            .collect()
    }

    /// The heuristic alone, without whatever depth model this machine
    /// happens to have installed.
    fn sky_of(rgba: &[f32], w: usize, h: usize, depth: Option<&[f32]>) -> Vec<f32> {
        let (rgb, sw, sh) = downsample(rgba, w, h, SKY_SIDE);
        assert_eq!((sw, sh), (w, h));
        sky(&rgb, w, h, depth)
    }

    #[test]
    fn sky_is_found_above_the_horizon_only() {
        let (w, h) = (80, 60);
        let c = sky_of(&landscape(w, h), w, h, None);
        assert!(c[5 * w + 40] > 0.9, "sky {}", c[5 * w + 40]);
        assert!(c[50 * w + 40] < 0.1, "ground {}", c[50 * w + 40]);
    }

    #[test]
    fn near_pixels_are_not_sky() {
        let (w, h) = (80, 60);
        let near = vec![1.0; w * h];
        let c = sky_of(&landscape(w, h), w, h, Some(&near));
        assert!(c.iter().all(|v| *v == 0.0));
    }

    #[test]
    fn a_picture_without_sky_finds_none() {
        let (w, h) = (40, 40);
        let ground: Vec<f32> = landscape(w, h * 2).split_off(w * h * 4);
        assert!(sky_of(&ground, w, h, None).iter().all(|v| *v == 0.0));
    }

    #[test]
    fn detection_stores_a_bounded_raster() {
        let (w, h) = (2000, 1000);
        let raster = detect(DetectedKind::Sky, &landscape(w, h), w, h).unwrap();
        assert_eq!((raster.width, raster.height), (512, 256));
        assert!(raster.is_valid());
    }

    #[test]
    fn masks_adjust_only_what_they_cover() {
        let (w, h) = (64, 32);
        let original = landscape(w, h);
        let mut rgba = original.clone();
        let mut mask = LocalMask::new(MaskShape::Linear {
            start: [0.0, 0.1],
            end: [0.0, 0.2],
        });
        mask.adjustments.exposure = -1.0;
        let mut masks = vec![mask];
        finish_raw(&mut rgba, w, h, &mut masks, true).unwrap();
        let g = |buf: &[f32], x: usize, y: usize| buf[(y * w + x) * 4 + 1];
        // The gradient is fully on above its start and off past its end.
        assert!(g(&rgba, 10, 20) == g(&original, 10, 20));
        assert!(g(&rgba, 10, 0) < g(&original, 10, 0) * 0.6);

        // A disabled or neutral mask does nothing.
        let mut rgba = original.clone();
        masks[0].enabled = false;
        finish_raw(&mut rgba, w, h, &mut masks, true).unwrap();
        assert_eq!(rgba, original);
    }

    #[test]
    fn detection_fills_only_missing_rasters_and_shares_one_run() {
        let (w, h) = (40, 30);
        let picture = landscape(w, h);
        let sky = || MaskShape::Detected {
            kind: DetectedKind::Sky,
            raster: None,
        };
        let mut masks = vec![LocalMask::new(sky()), LocalMask::new(sky())];
        detect_missing(&mut masks, &picture, w, h).unwrap();
        let raster = |m: &LocalMask| match &m.components[0].shape {
            MaskShape::Detected { raster, .. } => raster.clone().unwrap(),
            _ => unreachable!(),
        };
        assert!(Arc::ptr_eq(&raster(&masks[0]), &raster(&masks[1])));
        let before = raster(&masks[0]);
        detect_missing(&mut masks, &picture, w, h).unwrap();
        assert!(Arc::ptr_eq(&before, &raster(&masks[0])), "kept, not redone");
    }

    #[test]
    fn overlay_tints_by_coverage() {
        let mut px = vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        tint(&mut px, &[1.0, 0.0]);
        assert!((px[0] - 0.5).abs() < 1e-6 && px[4] == 0.0 && px[3] == 1.0);
    }

    #[test]
    fn local_values_carry_every_control() {
        let mut adjustments = LocalAdjustments::default();
        adjustments.set("dehaze", 40.0);
        adjustments.set("sharpness", -30.0);
        let values = local_values(&adjustments);
        assert_eq!(values.get("dehaze"), 40.0);
        assert_eq!(values.get("sharpness"), -30.0);
        assert_eq!(values.0.len(), LOCAL_CONTROLS.len());
    }
}
