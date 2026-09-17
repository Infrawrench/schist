//! General float-buffer kernels and device-resident multi-pass programs.
//! Inputs may be pixels, single-channel masks, sensors or tensors.

pub struct ComputeShader {
    pub name: &'static str,
    /// Implements `compute(index: u32)` using the shared bindings below.
    pub source: &'static str,
}

impl ComputeShader {
    pub fn wgsl(&self) -> String {
        format!("{COMPUTE_PRELUDE}\n{}", self.source)
    }
}

pub const COMPUTE_PRELUDE: &str = r#"
struct Shape { len: u32, width: u32, height: u32, channels: u32 }
@group(0) @binding(0) var<storage, read> src: array<f32>;
@group(0) @binding(1) var<storage, read> aux: array<f32>;
@group(0) @binding(2) var<storage, read_write> dst: array<f32>;
@group(0) @binding(3) var<storage, read> args: array<f32>;
@group(0) @binding(4) var<uniform> shape: Shape;
@compute @workgroup_size(256)
fn run_compute(@builtin(global_invocation_id) gid: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let i = gid.x + gid.y * groups.x * 256u;
    if i < shape.len { compute(i); }
}
"#;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComputeSource {
    /// Zero is the job's input; subsequent indices address `buffers`.
    Input(usize),
    /// An earlier step's output. Forward references are rejected.
    Step(usize),
}

#[derive(Clone)]
pub struct ComputeStep {
    pub shader: &'static ComputeShader,
    pub source: ComputeSource,
    pub auxiliary: ComputeSource,
    pub params: Vec<f32>,
    pub output_len: usize,
    /// Number of kernel invocations; a row kernel can write many samples.
    pub invocations: usize,
    /// Interpretation belongs to the kernel. The ABI supplies integer dimensions.
    pub shape: [u32; 3],
}

#[derive(Clone)]
pub struct ComputeProgram {
    pub buffers: Vec<Vec<f32>>,
    pub steps: Vec<ComputeStep>,
    pub result: ComputeSource,
    /// Estimated total work, used only for the production offload threshold.
    pub work: usize,
}

pub struct ComputeJob<'a> {
    pub input: &'a [f32],
    pub program: &'a ComputeProgram,
}

impl ComputeProgram {
    pub fn single(
        shader: &'static ComputeShader,
        params: Vec<f32>,
        len: usize,
        shape: [u32; 3],
        work: usize,
    ) -> Self {
        Self {
            buffers: vec![],
            steps: vec![ComputeStep {
                shader,
                source: ComputeSource::Input(0),
                auxiliary: ComputeSource::Input(0),
                params,
                output_len: len,
                invocations: len,
                shape,
            }],
            result: ComputeSource::Step(0),
            work,
        }
    }

    pub fn result_len(&self, input_len: usize) -> Option<usize> {
        self.source_len(self.result, input_len, self.steps.len())
    }

    fn source_len(&self, source: ComputeSource, input_len: usize, before: usize) -> Option<usize> {
        match source {
            ComputeSource::Input(0) => Some(input_len),
            ComputeSource::Input(i) => self.buffers.get(i - 1).map(Vec::len),
            ComputeSource::Step(i) if i < before => self.steps.get(i).map(|s| s.output_len),
            _ => None,
        }
    }

    pub fn valid(&self, input_len: usize) -> bool {
        !self.steps.is_empty()
            && self
                .result_len(input_len)
                .is_some_and(|len| len > 0 && len <= u32::MAX as usize)
            && self.steps.iter().enumerate().all(|(i, step)| {
                step.output_len > 0
                    && step.output_len <= u32::MAX as usize
                    && step.shape.iter().all(|&d| d > 0)
                    && step.invocations > 0
                    && step.invocations <= step.output_len
                    && self.source_len(step.source, input_len, i).is_some()
                    && self.source_len(step.auxiliary, input_len, i).is_some()
                    && step.params.iter().all(|v| v.is_finite())
            })
    }
}

/// Leaves the original input untouched on decline or failure.
pub fn try_compute(input: &[f32], program: &ComputeProgram) -> Option<Vec<f32>> {
    if !program.valid(input.len()) || !input.iter().all(|v| v.is_finite()) {
        return None;
    }
    let out = crate::backend().compute(&ComputeJob { input, program })?;
    (Some(out.len()) == program.result_len(input.len()) && out.iter().all(|v| v.is_finite()))
        .then_some(out)
}

pub static RGBA_BOX: ComputeShader = ComputeShader {
    name: "rgba-box-pass",
    source: include_str!("kernels/rgba_box.wgsl"),
};
impl ComputeProgram {
    pub fn push(
        &mut self,
        shader: &'static ComputeShader,
        source: ComputeSource,
        auxiliary: ComputeSource,
        params: Vec<f32>,
        len: usize,
        shape: [u32; 3],
    ) -> ComputeSource {
        let result = ComputeSource::Step(self.steps.len());
        self.steps.push(ComputeStep {
            shader,
            source,
            auxiliary,
            params,
            output_len: len,
            invocations: len,
            shape,
        });
        result
    }
    /// Append a premultiplied RGBA blur while retaining its input for later branches.
    pub fn rgba_blur(
        &mut self,
        source: ComputeSource,
        w: usize,
        h: usize,
        radius: usize,
        passes: usize,
    ) -> ComputeSource {
        let shape = [w as u32, h as u32, 4];
        let len = w * h * 4;
        let mut current = self.push(&RGBA_BOX, source, source, vec![0.0], len, shape);
        for _ in 0..passes {
            for axis in [1.0, 2.0] {
                current = self.push(
                    &RGBA_BOX,
                    current,
                    source,
                    vec![axis, radius as f32],
                    len,
                    shape,
                );
            }
        }
        self.push(&RGBA_BOX, current, source, vec![3.0], len, shape)
    }
}
