use super::*;
use schist_fx::{ComputeJob, ComputeShader, ComputeSource};

impl GpuContext {
    /// Upload inputs once, execute the graph on-device, read only its result.
    pub async fn run_compute_async(&self, job: &ComputeJob<'_>) -> Option<Vec<f32>> {
        let program = job.program;
        if !program.valid(job.input.len()) || self.is_lost() {
            return None;
        }
        let limit = self.binding_limit().min(BUDGET_BYTES);
        let bytes = |len: usize| len.max(1).checked_mul(4).filter(|&n| n <= limit);
        let mut total = bytes(job.input.len())?;
        for buffer in &program.buffers {
            total = total.checked_add(bytes(buffer.len())?)?;
        }
        // Reuse dead intermediates, never an input of the current dispatch.
        // Keep the result alive until the final copy into the staging buffer.
        let mut last_use: Vec<usize> = (0..program.steps.len()).collect();
        for (i, step) in program.steps.iter().enumerate() {
            for source in [step.source, step.auxiliary] {
                if let ComputeSource::Step(j) = source {
                    last_use[j] = last_use[j].max(i);
                }
            }
        }
        if let ComputeSource::Step(i) = program.result {
            last_use[i] = program.steps.len();
        }
        let mut slots: Vec<(usize, usize)> = Vec::new();
        let mut output_slots = Vec::new();
        for (i, step) in program.steps.iter().enumerate() {
            bytes(step.output_len)?;
            total = total.checked_add(bytes(step.params.len())?)?;
            let slot = slots
                .iter()
                .position(|&(len, until)| until < i && len >= step.output_len);
            let slot = match slot {
                Some(slot) => {
                    slots[slot].1 = last_use[i];
                    slot
                }
                None => {
                    slots.push((step.output_len, last_use[i]));
                    slots.len() - 1
                }
            };
            output_slots.push(slot);
        }
        for &(len, _) in &slots {
            total = total.checked_add(bytes(len)?)?;
        }
        total = total.checked_add(bytes(program.result_len(job.input.len())?)?)?;
        if total > BUDGET_BYTES {
            return None;
        }
        let max_groups = self.device.limits().max_compute_workgroups_per_dimension;
        let mut dispatches = Vec::new();
        for step in &program.steps {
            let groups = (step.invocations as u32).div_ceil(256);
            let x = groups.min(max_groups);
            let y = groups.div_ceil(x);
            if y > max_groups {
                return None;
            }
            dispatches.push((x, y));
        }
        let _work = self.work.lock().await;
        let mut pipelines = Vec::new();
        for step in &program.steps {
            let cached = self.compute_shaders.lock().get(step.shader.source).cloned();
            let pipeline = match cached {
                Some(p) => p?,
                None => {
                    let p = self.compile_compute(step.shader).await;
                    self.compute_shaders
                        .lock()
                        .insert(step.shader.source, p.clone());
                    p?
                }
            };
            pipelines.push(pipeline);
        }
        let mut scopes = ErrorScopes::default();
        scopes.push(self.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory));
        scopes.push(self.device.push_error_scope(wgpu::ErrorFilter::Validation));
        let upload = |data: &[f32]| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("compute-input"),
                    contents: crate::cast_f32s(if data.is_empty() { &[0.0] } else { data }),
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                })
        };
        let inputs: Vec<_> = std::iter::once(job.input)
            .chain(program.buffers.iter().map(Vec::as_slice))
            .map(upload)
            .collect();
        let outputs: Vec<_> = slots
            .iter()
            .map(|&(len, _)| {
                self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("compute-intermediate"),
                    size: (len * 4) as u64,
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                    mapped_at_creation: false,
                })
            })
            .collect();
        let source = |s: ComputeSource| match s {
            ComputeSource::Input(i) => &inputs[i],
            ComputeSource::Step(i) => &outputs[output_slots[i]],
        };
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("compute-program"),
            });
        for (i, step) in program.steps.iter().enumerate() {
            let args = upload(&step.params);
            let shape = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("compute-shape"),
                    contents: cast_u32s(&[
                        step.output_len as u32,
                        step.shape[0],
                        step.shape[1],
                        step.shape[2],
                    ]),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
            let bindings = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(step.shader.name),
                layout: &pipelines[i].get_bind_group_layout(0),
                entries: &[
                    bind_entry(0, source(step.source)),
                    bind_entry(1, source(step.auxiliary)),
                    bind_entry(2, &outputs[output_slots[i]]),
                    bind_entry(3, &args),
                    bind_entry(4, &shape),
                ],
            });
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some(step.shader.name),
                timestamp_writes: None,
            });
            pass.set_pipeline(&pipelines[i]);
            pass.set_bind_group(0, &bindings, &[]);
            pass.dispatch_workgroups(dispatches[i].0, dispatches[i].1, 1);
        }
        let len = program.result_len(job.input.len())?;
        let out = self
            .finish_fx(
                encoder,
                source(program.result),
                (len * 4) as u64,
                "compute program",
                &mut scopes,
            )
            .await;
        if let Some(error) = scopes.pop().await {
            log::warn!("GPU program allocation failed: {error}");
            return None;
        }
        out
    }

    async fn compile_compute(&self, shader: &ComputeShader) -> Option<wgpu::ComputePipeline> {
        let mut scopes = ErrorScopes::default();
        scopes.push(self.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory));
        scopes.push(self.device.push_error_scope(wgpu::ErrorFilter::Validation));
        let module = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(shader.name),
                source: wgpu::ShaderSource::Wgsl(shader.wgsl().into()),
            });
        let bindings = self
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some(shader.name),
                entries: &(0..5)
                    .map(|binding| wgpu::BindGroupLayoutEntry {
                        binding,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: if binding == 4 {
                                wgpu::BufferBindingType::Uniform
                            } else {
                                wgpu::BufferBindingType::Storage {
                                    read_only: binding != 2,
                                }
                            },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    })
                    .collect::<Vec<_>>(),
            });
        let layout = self
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(shader.name),
                bind_group_layouts: &[Some(&bindings)],
                immediate_size: 0,
            });
        let pipeline = self
            .device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(shader.name),
                layout: Some(&layout),
                module: &module,
                entry_point: Some("run_compute"),
                compilation_options: Default::default(),
                cache: None,
            });
        let validation = scopes.pop().await;
        let allocation = scopes.pop().await;
        if let Some(error) = validation.or(allocation) {
            log::warn!("GPU kernel {} unavailable: {error}", shader.name);
            return None;
        }
        Some(pipeline)
    }
}
