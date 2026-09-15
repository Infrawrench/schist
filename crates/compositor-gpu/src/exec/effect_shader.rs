use super::*;
use schist_fx::{ShaderJob, ShaderSpec};

impl GpuContext {
    /// Execute an effect-owned shader. Pipelines (including failed
    /// compilations) are cached by source, so names need not be unique.
    /// The cost threshold lives in GpuFx; tests can exercise tiny jobs here.
    pub fn run_shader(&self, job: &ShaderJob<'_>) -> Option<Vec<f32>> {
        if !job.valid() {
            return None;
        }
        let limits = self.device.limits();
        let row_bytes = job.width.checked_mul(16)?;
        let limit = self
            .binding_limit()
            .min(limits.max_storage_buffer_binding_size as usize)
            .min(usize::try_from(limits.max_buffer_size).unwrap_or(usize::MAX));
        let max_rows =
            (limit / row_bytes).min(limits.max_compute_workgroups_per_dimension as usize * 16);
        if job.width > i32::MAX as usize
            || job.height > i32::MAX as usize
            || job.width.div_ceil(16) > limits.max_compute_workgroups_per_dimension as usize
            || std::mem::size_of_val(job.params).max(4) > limit
        {
            return None;
        }
        let (band_rows, halo) = if job.height <= max_rows {
            (job.height, 0)
        } else {
            let halo = job.halo?;
            let rows = max_rows.checked_sub(halo.checked_mul(2)?)?;
            if rows == 0 {
                return None;
            }
            (rows, halo)
        };
        let _work = self.work.lock();
        let pipeline = {
            let mut cache = self.effect_shaders.lock();
            cache
                .entry(job.shader.source)
                .or_insert_with(|| self.compile_effect(job.shader))
                .clone()?
        };
        let mut out = vec![0.0; job.px.len()];
        for top in (0..job.height).step_by(band_rows) {
            let bottom = (top + band_rows).min(job.height);
            let first = top.saturating_sub(halo);
            let end = bottom.saturating_add(halo).min(job.height);
            let px = &job.px[first * job.width * 4..end * job.width * 4];
            let band = self.effect_band(job, &pipeline, px, first, end - first)?;
            out[top * job.width * 4..bottom * job.width * 4].copy_from_slice(
                &band[(top - first) * job.width * 4..(bottom - first) * job.width * 4],
            );
        }
        Some(out)
    }

    fn compile_effect(&self, shader: &ShaderSpec) -> Option<wgpu::ComputePipeline> {
        self.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(shader.name),
                source: wgpu::ShaderSource::Wgsl(shader.wgsl().into()),
            });
        // Explicit layout: a shader may not use args or read the source,
        // but every effect still binds the same four resources.
        let bind_layout = self
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some(shader.name),
                entries: &(0..4)
                    .map(|binding| wgpu::BindGroupLayoutEntry {
                        binding,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: if binding == 0 {
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
                bind_group_layouts: &[&bind_layout],
                push_constant_ranges: &[],
            });
        let pipeline = self
            .device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(shader.name),
                layout: Some(&layout),
                module: &module,
                entry_point: Some("run_effect"),
                compilation_options: Default::default(),
                cache: None,
            });
        let validation = pollster::block_on(self.device.pop_error_scope());
        let allocation = pollster::block_on(self.device.pop_error_scope());
        if let Some(error) = validation.or(allocation) {
            log::warn!("effect shader {} unavailable: {error}", shader.name);
            return None;
        }
        Some(pipeline)
    }

    fn effect_band(
        &self,
        job: &ShaderJob<'_>,
        pipeline: &wgpu::ComputePipeline,
        px: &[f32],
        first: usize,
        rows: usize,
    ) -> Option<Vec<f32>> {
        self.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let upload = |label, data: &[u8], usage| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(label),
                    contents: data,
                    usage,
                })
        };
        let src = upload(
            "effect-source",
            crate::fx::cast_f32s(px),
            wgpu::BufferUsages::STORAGE,
        );
        let dimensions = upload(
            "effect-image",
            cast_u32s(&[
                job.width as u32,
                job.height as u32,
                first as u32,
                rows as u32,
            ]),
            wgpu::BufferUsages::UNIFORM,
        );
        let args = upload(
            "effect-args",
            crate::fx::cast_f32s(if job.params.is_empty() {
                &[0.0]
            } else {
                job.params
            }),
            wgpu::BufferUsages::STORAGE,
        );
        let bytes = std::mem::size_of_val(px) as u64;
        let dst = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("effect-result"),
            size: bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(job.shader.name),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                bind_entry(0, &dimensions),
                bind_entry(1, &src),
                bind_entry(2, &dst),
                bind_entry(3, &args),
            ],
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some(job.shader.name),
            });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some(job.shader.name),
                timestamp_writes: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(
                (job.width as u32).div_ceil(16),
                (rows as u32).div_ceil(16),
                1,
            );
        }
        let result = self.finish_fx(encoder, &dst, bytes, job.shader.name);
        if let Some(error) = pollster::block_on(self.device.pop_error_scope()) {
            log::warn!("effect {} allocation failed: {error}", job.shader.name);
            return None;
        }
        result
    }
}
