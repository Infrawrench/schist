//! General float-buffer kernels and device-resident multi-pass programs.
//! Inputs may be pixels, single-channel masks, sensors or tensors.

#[derive(Clone, Copy)]
pub struct ComputeShader {
    pub name: &'static str,
    /// Implements `compute(index: u32)` using the shared bindings below.
    pub source: &'static str,
    pub entry: ComputeEntry,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ComputeEntry {
    /// One call to `compute(index)` per output element.
    Element,
    /// One RGBA pixel through the effect shader ABI.
    Rgba,
    /// Atomic writes to a zeroed destination; values use explicit u32 bit patterns.
    Atomic,
    /// One cooperative call to `compute_group(group, lane)` per 256-thread
    /// workgroup. The bounds check is uniform, so barriers are legal.
    Workgroup,
}

impl ComputeShader {
    pub const fn new(name: &'static str, source: &'static str) -> Self {
        Self {
            name,
            source,
            entry: ComputeEntry::Element,
        }
    }

    pub const fn workgroup(name: &'static str, source: &'static str) -> Self {
        Self {
            name,
            source,
            entry: ComputeEntry::Workgroup,
        }
    }

    pub fn wgsl(&self) -> String {
        let entry = match self.entry {
            ComputeEntry::Element => COMPUTE_ENTRY,
            ComputeEntry::Atomic => {
                return format!("{ATOMIC_BINDINGS}\n{COMPUTE_ENTRY}\n{}", self.source)
            }
            ComputeEntry::Rgba => {
                return format!(
                    "{COMPUTE_BINDINGS}\n{}\n{}\n{}",
                    include_str!("shader_compute.wgsl"),
                    include_str!("shader.wgsl"),
                    self.source
                )
            }
            ComputeEntry::Workgroup => WORKGROUP_ENTRY,
        };
        format!("{COMPUTE_BINDINGS}\n{entry}\n{}", self.source)
    }
}

pub const COMPUTE_BINDINGS: &str = r#"
struct Shape { len: u32, width: u32, height: u32, channels: u32 }
@group(0) @binding(0) var<storage, read> src: array<f32>;
@group(0) @binding(1) var<storage, read> aux: array<f32>;
@group(0) @binding(2) var<storage, read_write> dst: array<f32>;
@group(0) @binding(3) var<storage, read> args: array<f32>;
@group(0) @binding(4) var<uniform> shape: Shape;
"#;

const ATOMIC_BINDINGS: &str = r#"
struct Shape { len: u32, width: u32, height: u32, channels: u32 }
@group(0) @binding(0) var<storage, read> src: array<f32>;
@group(0) @binding(1) var<storage, read> aux: array<f32>;
@group(0) @binding(2) var<storage, read_write> dst: array<atomic<u32>>;
@group(0) @binding(3) var<storage, read> args: array<f32>;
@group(0) @binding(4) var<uniform> shape: Shape;
"#;

pub const COMPUTE_ENTRY: &str = r#"
@compute @workgroup_size(256)
fn run_compute(@builtin(global_invocation_id) gid: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let i = gid.x + gid.y * groups.x * 256u;
    if i < shape.len { compute(i); }
}
"#;

const WORKGROUP_ENTRY: &str = r#"
@compute @workgroup_size(256)
fn run_compute(
    @builtin(workgroup_id) group: vec3<u32>,
    @builtin(local_invocation_index) lane: u32,
    @builtin(num_workgroups) groups: vec3<u32>,
) {
    let i = group.x + group.y * groups.x;
    if i < shape.len {
        compute_group(i, lane);
    }
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
    pub shader: ComputeShader,
    pub source: ComputeSource,
    pub auxiliary: ComputeSource,
    pub params: Vec<f32>,
    pub output_len: usize,
    /// Number of elements or cooperative workgroups, according to the shader.
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
    /// Append another program with its primary input bound to an existing
    /// source. Intermediates stay resident and their last uses remain visible
    /// to the allocator, including branches that refer to the original input.
    pub fn append(&mut self, program: &Self, input: ComputeSource) -> ComputeSource {
        let buffer_offset = self.buffers.len();
        let step_offset = self.steps.len();
        let remap = |source| match source {
            ComputeSource::Input(0) => input,
            ComputeSource::Input(i) => ComputeSource::Input(buffer_offset + i),
            ComputeSource::Step(i) => ComputeSource::Step(step_offset + i),
        };
        self.buffers.extend(program.buffers.iter().cloned());
        self.steps
            .extend(program.steps.iter().cloned().map(|mut step| {
                step.source = remap(step.source);
                step.auxiliary = remap(step.auxiliary);
                step
            }));
        self.work = self.work.saturating_add(program.work);
        remap(program.result)
    }

    pub fn single(
        shader: &ComputeShader,
        params: Vec<f32>,
        len: usize,
        shape: [u32; 3],
        work: usize,
    ) -> Self {
        Self {
            buffers: vec![],
            steps: vec![ComputeStep {
                shader: *shader,
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
                    && step.invocations <= u32::MAX as usize
                    && (step.shader.entry == ComputeEntry::Atomic
                        || step.invocations <= step.output_len)
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

pub static RGBA_BOX: ComputeShader =
    ComputeShader::new("rgba-box-pass", include_str!("kernels/rgba_box.wgsl"));
impl ComputeProgram {
    pub fn push(
        &mut self,
        shader: &ComputeShader,
        source: ComputeSource,
        auxiliary: ComputeSource,
        params: Vec<f32>,
        len: usize,
        shape: [u32; 3],
    ) -> ComputeSource {
        let result = ComputeSource::Step(self.steps.len());
        self.steps.push(ComputeStep {
            shader: *shader,
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

/// Awaitable executor for browser/library callers. Futures may remain on the
/// foreground thread because browser WebGPU resources are not thread-safe.
pub trait AsyncCompute {
    fn compute_async<'a>(
        &'a self,
        job: ComputeJob<'a>,
    ) -> impl std::future::Future<Output = Option<Vec<f32>>> + 'a;
}
