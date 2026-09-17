use super::*;
use schist_fx::{ShaderJob, ShaderSpec};

impl GpuContext {
    /// Execute an effect-owned shader. Pipelines (including failed
    /// compilations) are cached by source, so names need not be unique.
    /// The cost threshold lives in GpuFx; tests can exercise tiny jobs here.
    pub async fn run_shader_async(&self, job: &ShaderJob<'_>) -> Option<Vec<f32>> {
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
        if max_rows == 0 {
            return None;
        }
        let local_rows = job
            .halo
            .and_then(|halo| max_rows.checked_sub(halo.checked_mul(2)?));
        let paged = job.height > max_rows && local_rows.is_none_or(|rows| rows == 0);
        let (band_rows, halo) = if job.height <= max_rows {
            (job.height, 0)
        } else if paged {
            (max_rows, 0)
        } else {
            (local_rows?, job.halo?)
        };
        // Include texture padding, output and staging in the allocation budget.
        // Upload before taking the execution lock; the upload owns that lock too.
        let source = if paged {
            let pixels = job.px.len() / 4;
            let edge = limits.max_texture_dimension_2d.min(1024) as usize;
            let width = pixels.min(edge);
            let page = width * pixels.div_ceil(width).min(edge);
            let bytes = pixels.div_ceil(page).checked_mul(page)?.checked_mul(16)?;
            if bytes
                .checked_add(band_rows.checked_mul(row_bytes)?.checked_mul(2)?)?
                .checked_add(std::mem::size_of_val(job.params))?
                > BUDGET_BYTES
            {
                return None;
            }
            Some(self.upload_warp_source_async(job.px).await?)
        } else {
            None
        };
        let _work = self.work.lock().await;
        let cached = self
            .effect_shaders
            .lock()
            .get(&(job.shader.source, paged))
            .cloned();
        let pipeline = match cached {
            Some(pipeline) => pipeline?,
            None => {
                let pipeline = self.compile_effect(job.shader, paged).await;
                self.effect_shaders
                    .lock()
                    .insert((job.shader.source, paged), pipeline.clone());
                pipeline?
            }
        };
        let mut out = vec![0.0; job.px.len()];
        for top in (0..job.height).step_by(band_rows) {
            let bottom = (top + band_rows).min(job.height);
            let first = top.saturating_sub(halo);
            let end = bottom.saturating_add(halo).min(job.height);
            let px = &job.px[first * job.width * 4..end * job.width * 4];
            let band = self
                .effect_band(job, &pipeline, px, first, end - first, source.as_ref())
                .await?;
            out[top * job.width * 4..bottom * job.width * 4].copy_from_slice(
                &band[(top - first) * job.width * 4..(bottom - first) * job.width * 4],
            );
        }
        Some(out)
    }

    async fn compile_effect(
        &self,
        shader: &ShaderSpec,
        paged: bool,
    ) -> Option<wgpu::ComputePipeline> {
        let mut scopes = ErrorScopes::default();
        scopes.push(self.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory));
        scopes.push(self.device.push_error_scope(wgpu::ErrorFilter::Validation));
        let module = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(shader.name),
                source: wgpu::ShaderSource::Wgsl(
                    if paged {
                        shader.wgsl_paged()
                    } else {
                        shader.wgsl()
                    }
                    .into(),
                ),
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
                        ty: if binding == 1 && paged {
                            wgpu::BindingType::Texture {
                                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                                view_dimension: wgpu::TextureViewDimension::D2Array,
                                multisampled: false,
                            }
                        } else {
                            wgpu::BindingType::Buffer {
                                ty: if binding == 0 {
                                    wgpu::BufferBindingType::Uniform
                                } else {
                                    wgpu::BufferBindingType::Storage {
                                        read_only: binding != 2,
                                    }
                                },
                                has_dynamic_offset: false,
                                min_binding_size: None,
                            }
                        },
                        count: None,
                    })
                    .collect::<Vec<_>>(),
            });
        let layout = self
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(shader.name),
                bind_group_layouts: &[Some(&bind_layout)],
                immediate_size: 0,
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
        let validation = scopes.pop().await;
        let allocation = scopes.pop().await;
        if let Some(error) = validation.or(allocation) {
            log::warn!("effect shader {} unavailable: {error}", shader.name);
            return None;
        }
        Some(pipeline)
    }

    async fn effect_band(
        &self,
        job: &ShaderJob<'_>,
        pipeline: &wgpu::ComputePipeline,
        px: &[f32],
        first: usize,
        rows: usize,
        source: Option<&WarpSource>,
    ) -> Option<Vec<f32>> {
        let mut scopes = ErrorScopes::default();
        scopes.push(self.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory));
        scopes.push(self.device.push_error_scope(wgpu::ErrorFilter::Validation));
        let upload = |label, data: &[u8], usage| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(label),
                    contents: data,
                    usage,
                })
        };
        let src = source.is_none().then(|| {
            upload(
                "effect-source",
                crate::cast_f32s(px),
                wgpu::BufferUsages::STORAGE,
            )
        });
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
            crate::cast_f32s(if job.params.is_empty() {
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
                match source {
                    Some(source) => wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&source.view),
                    },
                    None => bind_entry(1, src.as_ref()?),
                },
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
        let result = self
            .finish_fx(encoder, &dst, bytes, job.shader.name, &mut scopes)
            .await;
        if let Some(error) = scopes.pop().await {
            log::warn!("effect {} allocation failed: {error}", job.shader.name);
            return None;
        }
        result
    }
}
