//! Full RGBA filter graphs: mirrored tile preparation, inference, decoding and
//! overlap trimming share a submission and one immutable set of model weights.
use crate::{Input, Model, Range};
use schist_fx::{ComputeProgram, ComputeShader, ComputeSource as Source, FilterOperation};
use std::sync::Arc;
static IMAGE: ComputeShader = ComputeShader::new("neural-image", include_str!("gpu_image.wgsl"));

impl Model {
    pub fn rgba_operation(self: &Arc<Self>, blend: f32) -> Option<FilterOperation> {
        self.gpu.as_ref()?;
        if !matches!(self.spec.input, Input::Tiles { scale: 1, .. })
            || self.channels != 3
            || self.nhwc
        {
            return None;
        }
        let model = self.clone();
        Some(FilterOperation::Captured {
            work_per_pixel: 4096,
            build: Arc::new(move |w, h| model.rgba_program(w, h, blend)),
        })
    }

    fn rgba_program(&self, w: usize, h: usize, blend: f32) -> Option<ComputeProgram> {
        let Input::Tiles {
            size: t,
            overlap,
            scale: 1,
        } = self.spec.input
        else {
            return None;
        };
        let overlap = overlap.min(t / 4);
        let step = t.checked_sub(overlap * 2).filter(|&n| n > 0)?;
        let len = w
            .checked_mul(h)?
            .checked_mul(4)
            .filter(|&n| n > 0 && n <= 16_777_216)?;
        if w.div_ceil(step).checked_mul(h.div_ceil(step))? > 256 {
            return None;
        }
        let net = self.gpu.as_ref()?;
        if net.shapes.len() != 1
            || net.shapes[0].len() != 4
            || net.shapes[0][0] != 1
            || net.shapes[0][1] < 3
        {
            return None;
        }
        let shape = &net.shapes[0];
        let (mean, sd) = match self.spec.range {
            Range::Unit => ([0.0; 3], [1.0; 3]),
            Range::Byte => ([0.0; 3], [1.0 / 255.0; 3]),
            Range::Standard { mean, sd } => (mean, sd),
        };
        let mut p = ComputeProgram {
            buffers: net.program.buffers.clone(),
            steps: vec![],
            result: Source::Input(0),
            work: 0,
        };
        for y in (0..h).step_by(step) {
            for x in (0..w).step_by(step) {
                let mut args = vec![
                    0.0,
                    w as f32,
                    h as f32,
                    t as f32,
                    overlap as f32,
                    x as f32,
                    y as f32,
                    step as f32,
                    shape[3] as f32,
                    shape[2] as f32,
                    blend.clamp(0.0, 1.0),
                ];
                args.extend(mean);
                args.extend(sd);
                args.push(u8::from(matches!(self.spec.range, Range::Byte)) as f32);
                let patch = p.push(
                    &IMAGE,
                    Source::Input(0),
                    Source::Input(0),
                    args.clone(),
                    t * t * 3,
                    [t as u32, t as u32, 3],
                );
                let offset = p.steps.len();
                let remap = |source| match source {
                    Source::Input(0) => patch,
                    Source::Step(i) => Source::Step(i + offset),
                    other => other,
                };
                for stage in &net.program.steps {
                    let mut stage = stage.clone();
                    stage.source = remap(stage.source);
                    stage.auxiliary = remap(stage.auxiliary);
                    p.steps.push(stage);
                }
                let inferred = remap(net.program.result);
                args[0] = 1.0;
                p.result = p.push(
                    &IMAGE,
                    p.result,
                    inferred,
                    args,
                    len,
                    [w as u32, h as u32, 4],
                );
                p.work = p.work.saturating_add(net.program.work).saturating_add(len);
            }
        }
        Some(p)
    }
}
