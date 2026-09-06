//! Large-layer seam carving. Texture arrays hold the image planes; the
//! cumulative-cost scan keeps just its previous and next boundary rows.
use super::*;

pub(super) struct Pipelines {
    stages: Vec<wgpu::ComputePipeline>,
}

const ENTRIES: &[(&str, &[u32])] = &[
    ("energy_pass", &[0, 1, 3, 5]),
    ("dp_seed", &[0, 6, 7, 9]),
    ("dp_tile", &[0, 6, 7, 8, 9]),
    ("pick", &[0, 6, 10]),
    ("resample", &[0, 1, 2, 3, 4]),
    ("advance_seam", &[0]),
];

struct Plane {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    width: u32,
    height: u32,
    layers: u32,
    channels: usize,
}

impl Plane {
    fn new(
        device: &wgpu::Device,
        size: wgpu::Extent3d,
        format: wgpu::TextureFormat,
        channels: usize,
    ) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("carve-paged-plane"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        Self {
            texture,
            view,
            width: size.width,
            height: size.height,
            layers: size.depth_or_array_layers,
            channels,
        }
    }

    fn upload(
        &self,
        queue: &wgpu::Queue,
        data: &[f32],
        width: usize,
        stride: usize,
        height: usize,
    ) {
        let page = self.width as usize * self.height as usize;
        for layer in 0..self.layers {
            let start = layer as usize * page;
            let end = (start + page).min(stride * height);
            let mut pixels = vec![0.0; page * self.channels];
            for y in start / stride..end.div_ceil(stride) {
                let from = start.max(y * stride);
                let to = end.min(y * stride + width);
                if from < to {
                    let dst = (from - start) * self.channels;
                    let src = (y * width + from - y * stride) * self.channels;
                    let len = (to - from) * self.channels;
                    pixels[dst..dst + len].copy_from_slice(&data[src..src + len]);
                }
            }
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: 0,
                        y: 0,
                        z: layer,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                crate::fx::cast_f32s(&pixels),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.width * self.channels as u32 * 4),
                    rows_per_image: None,
                },
                wgpu::Extent3d {
                    width: self.width,
                    height: self.height,
                    depth_or_array_layers: 1,
                },
            );
        }
    }

    fn read(&self, ctx: &GpuContext, count: usize) -> Option<Vec<f32>> {
        let row_bytes = self.width as usize * self.channels * 4;
        let padded_row = row_bytes.div_ceil(256) * 256;
        let bytes = (padded_row * self.height as usize) as u64;
        let staging = ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("carve-page-readback"),
            size: bytes,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut out = Vec::with_capacity(count * self.channels);
        for layer in 0..self.layers {
            let mut encoder = ctx.device.create_command_encoder(&Default::default());
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: 0,
                        y: 0,
                        z: layer,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &staging,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(padded_row as u32),
                        rows_per_image: None,
                    },
                },
                wgpu::Extent3d {
                    width: self.width,
                    height: self.height,
                    depth_or_array_layers: 1,
                },
            );
            ctx.queue.submit([encoder.finish()]);
            let slice = staging.slice(..);
            let (tx, rx) = std::sync::mpsc::channel();
            slice.map_async(wgpu::MapMode::Read, move |r| {
                let _ = tx.send(r);
            });
            ctx.device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
            rx.recv().ok()?.ok()?;
            let data = slice.get_mapped_range();
            for row in data.chunks(padded_row) {
                let remaining = count * self.channels - out.len();
                out.extend(
                    row[..row_bytes]
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .take(remaining)
                        .map(|b| f32::from_le_bytes(*b)),
                );
            }
            drop(data);
            staging.unmap();
        }
        Some(out)
    }
}

impl GpuContext {
    /// Seam carving without full-image storage-buffer bindings. Also
    /// public so parity tests can exercise this path on small fixtures.
    pub fn run_carve_paged(&self, job: &schist_fx::CarveJob<'_>) -> Option<schist_fx::Carved> {
        self.run_carve_paged_with_edge(job, 1024)
    }

    /// A smaller texture page edge can bound transient upload/readback
    /// allocations. It also makes cross-page tests affordable.
    pub fn run_carve_paged_with_edge(
        &self,
        job: &schist_fx::CarveJob<'_>,
        edge: u32,
    ) -> Option<schist_fx::Carved> {
        let (w, h) = (job.width, job.height);
        let target = job.target_width.max(1);
        if w == 0 || h == 0 || w == target || edge == 0 {
            return None;
        }
        let stride = w.max(target);
        let count = stride.checked_mul(h)?;
        let count32 = u32::try_from(count).ok()?;
        if job.px.len() != w.checked_mul(h)?.checked_mul(4)? || job.protect.len() != w * h {
            return None;
        }
        let limits = self.device.limits();
        let edge = edge.min(1024).min(limits.max_texture_dimension_2d);
        let tw = count32.min(edge);
        let th = count32.div_ceil(tw).min(edge);
        let layers = count32.div_ceil(tw * th);
        if layers > limits.max_texture_array_layers
            || stride.checked_mul(8)? > self.binding_limit()
            || h.checked_add(8)?.checked_mul(4)? > self.binding_limit()
            || stride.div_ceil(16).max(h.div_ceil(16))
                > limits.max_compute_workgroups_per_dimension as usize
        {
            return None;
        }
        let _work = self.work.lock();
        self.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        // Always balance the device's error scopes, even if readback fails.
        let result = (|| {
            let pipelines = self.paged_carve.get_or_init(|| {
                let module = self
                    .device
                    .create_shader_module(wgpu::ShaderModuleDescriptor {
                        label: Some("fx_carve_paged.wgsl"),
                        source: wgpu::ShaderSource::Wgsl(
                            include_str!("../fx_carve_paged.wgsl").into(),
                        ),
                    });
                Pipelines {
                    stages: ENTRIES
                        .iter()
                        .map(|(entry, _)| {
                            self.device
                                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                                    label: Some(entry),
                                    layout: None,
                                    module: &module,
                                    entry_point: Some(entry),
                                    compilation_options: Default::default(),
                                    cache: None,
                                })
                        })
                        .collect(),
                }
            });
            let size = wgpu::Extent3d {
                width: tw,
                height: th,
                depth_or_array_layers: layers,
            };
            let plane = |format, channels| Plane::new(&self.device, size, format, channels);
            let px = [
                plane(wgpu::TextureFormat::Rgba32Float, 4),
                plane(wgpu::TextureFormat::Rgba32Float, 4),
            ];
            let protect = [
                plane(wgpu::TextureFormat::R32Float, 1),
                plane(wgpu::TextureFormat::R32Float, 1),
            ];
            let energy = plane(wgpu::TextureFormat::R32Float, 1);
            let directions = plane(wgpu::TextureFormat::R32Sint, 1);
            px[0].upload(&self.queue, job.px, w, stride, h);
            protect[0].upload(&self.queue, job.protect, w, stride, h);
            let mut init = vec![0; h + 8];
            init[..4].copy_from_slice(&[w as u32, h as u32, stride as u32, u32::from(target > w)]);
            init[6] = tw;
            init[7] = th;
            let state = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("carve-paged-state"),
                    contents: cast_u32s(&init),
                    usage: wgpu::BufferUsages::STORAGE,
                });
            let cost = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("carve-boundary-costs"),
                size: (stride * 8) as u64,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            });
            let tiles = (h - 1).div_ceil(CARVE_TILE_ROWS);
            let mut bands = vec![0; tiles.max(1) * UNIFORM_ALIGN / 4];
            for i in 0..tiles {
                bands[i * UNIFORM_ALIGN / 4] = (i * CARVE_TILE_ROWS) as u32;
            }
            let bands = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("carve-paged-bands"),
                    contents: cast_u32s(&bands),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
            let bind = |stage: usize, direction: usize, band: usize| {
                let resources = [
                    state.as_entire_binding(),
                    wgpu::BindingResource::TextureView(&px[direction].view),
                    wgpu::BindingResource::TextureView(&px[1 - direction].view),
                    wgpu::BindingResource::TextureView(&protect[direction].view),
                    wgpu::BindingResource::TextureView(&protect[1 - direction].view),
                    wgpu::BindingResource::TextureView(&energy.view),
                    cost.as_entire_binding(),
                    wgpu::BindingResource::TextureView(&directions.view),
                    wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &bands,
                        offset: (band * UNIFORM_ALIGN) as u64,
                        size: wgpu::BufferSize::new(16),
                    }),
                    wgpu::BindingResource::TextureView(&energy.view),
                    wgpu::BindingResource::TextureView(&directions.view),
                ];
                let entries: Vec<_> = ENTRIES[stage]
                    .1
                    .iter()
                    .map(|&binding| wgpu::BindGroupEntry {
                        binding,
                        resource: resources[binding as usize].clone(),
                    })
                    .collect();
                self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some(ENTRIES[stage].0),
                    layout: &pipelines.stages[stage].get_bind_group_layout(0),
                    entries: &entries,
                })
            };
            let energy_binds = [bind(0, 0, 0), bind(0, 1, 0)];
            let seed = bind(1, 0, 0);
            let tile_binds: Vec<_> = (0..tiles).map(|i| bind(2, 0, i)).collect();
            let pick = bind(3, 0, 0);
            let resample = [bind(4, 0, 0), bind(4, 1, 0)];
            let advance = bind(5, 0, 0);
            let seams = w.abs_diff(target);
            for first in (0..seams).step_by(CARVE_SEAMS_PER_SUBMIT) {
                let mut encoder = self.device.create_command_encoder(&Default::default());
                {
                    let mut pass = encoder.begin_compute_pass(&Default::default());
                    for seam in first..(first + CARVE_SEAMS_PER_SUBMIT).min(seams) {
                        let mut dispatch =
                            |stage: usize, group: &wgpu::BindGroup, x: usize, y: usize| {
                                pass.set_pipeline(&pipelines.stages[stage]);
                                pass.set_bind_group(0, group, &[]);
                                pass.dispatch_workgroups(x as u32, y as u32, 1);
                            };
                        dispatch(
                            0,
                            &energy_binds[seam % 2],
                            stride.div_ceil(16),
                            h.div_ceil(16),
                        );
                        dispatch(1, &seed, stride.div_ceil(CARVE_WG), 1);
                        for tile in &tile_binds {
                            dispatch(2, tile, stride.div_ceil(CARVE_TILE_COLS), 1);
                        }
                        dispatch(3, &pick, 1, 1);
                        dispatch(4, &resample[seam % 2], stride.div_ceil(16), h.div_ceil(16));
                        dispatch(5, &advance, 1, 1);
                    }
                }
                self.queue.submit([encoder.finish()]);
            }
            let px = px[seams % 2].read(self, count)?;
            let protect = protect[seams % 2].read(self, count)?;
            Some(schist_fx::Carved {
                px: unpad_rows(&px, stride * 4, target * 4, h),
                protect: unpad_rows(&protect, stride, target, h),
                width: target,
            })
        })();
        let validation = pollster::block_on(self.device.pop_error_scope());
        let allocation = pollster::block_on(self.device.pop_error_scope());
        if let Some(error) = validation.or(allocation) {
            log::warn!("GPU paged carve failed: {error}");
            return None;
        }
        result
    }
}
