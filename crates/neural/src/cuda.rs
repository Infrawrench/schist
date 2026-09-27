//! NVIDIA inference with Rust graph execution and embedded PTX. The only
//! dynamically loaded library is the system NVIDIA driver, never a bundled
//! inference runtime. Unsupported graphs retain the existing portable path.
mod driver;
mod graph;
mod memory;
#[cfg(test)]
mod tests;
use anyhow::{ensure, Context, Result};
use driver::{Device, Ptr};
use graph::Graph;
use std::sync::{Arc, Mutex};
use tract_onnx::prelude::*;

// NUL termination is required by cuModuleLoadData for PTX text.
static PTX: &str = concat!(include_str!("cuda/kernels.ptx"), "\0");

pub(super) struct Network {
    session: Mutex<Option<Session>>,
}
struct Dispatch {
    kernel: usize,
    blocks: u32,
    pointers: [Ptr; 4],
    len: u32,
}
struct Session {
    device: Arc<Device>,
    allocations: Vec<Ptr>,
    input: Ptr,
    input_shape: Vec<usize>,
    error: Ptr,
    steps: Vec<Dispatch>,
    outputs: Vec<(Ptr, Vec<usize>)>,
}
impl Network {
    pub fn load(id: &str, model: &TypedModel) -> Option<Self> {
        // Enable explicitly until the hardware parity suite has passed on NVIDIA.
        if !std::env::var_os("SCHIST_NEURAL_CUDA").is_some_and(|v| v == "1")
            || !matches!(
                id,
                "foreground"
                    | "foreground-matting"
                    | "detail-matting"
                    | "subject-guide"
                    | "matting"
            )
        {
            return None;
        }
        let result = (|| -> Result<Self> {
            let device = Device::get()?;
            let graph = Graph::compile(model)?;
            let session = Session::new(device, graph)?;
            log::info!(target:"schist_neural::execution","{id}: CUDA resident graph, {} dispatches on {}",session.steps.len(),session.device.name);
            Ok(Self {
                session: Mutex::new(Some(session)),
            })
        })();
        match result {
            Ok(network) => Some(network),
            Err(error) => {
                log::info!(target:"schist_neural::execution","{id}: CUDA unavailable; using portable inference: {error:#}");
                None
            }
        }
    }
    pub fn active(&self) -> bool {
        self.session
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some()
    }
    pub fn run(&self, inputs: &TVec<TValue>) -> Option<TVec<TValue>> {
        let mut session = self.session.lock().unwrap_or_else(|e| e.into_inner());
        match session.as_ref()?.run(inputs) {
            Ok(result) => Some(result),
            Err(error) => {
                log::warn!(target:"schist_neural::execution","CUDA disabled for this model after failure; using portable inference: {error:#}");
                *session = None;
                None
            }
        }
    }
}
impl Session {
    fn new(device: Arc<Device>, graph: Graph) -> Result<Self> {
        let layout = memory::Layout::plan(&graph)?;
        let guard = device.enter()?;
        // Pack parameters and 64-bit pointer tables together. Thousands of tiny
        // driver allocations would otherwise inflate setup cost and VRAM use.
        let metadata_words = graph
            .steps
            .iter()
            .try_fold(0usize, |sum, s| {
                sum.checked_add(s.params.len().next_multiple_of(2) + 32)
            })
            .context("CUDA metadata size overflow")?;
        let metadata_bytes = metadata_words
            .checked_mul(4)
            .context("CUDA metadata size overflow")?;
        let bytes = layout
            .bytes()?
            .checked_add(metadata_bytes)
            .and_then(|v| v.checked_add(4))
            .context("CUDA graph size overflow")?;
        ensure!(
            bytes <= guard.free_bytes()?.saturating_sub(256 * 1024 * 1024) * 3 / 4,
            "CUDA graph exceeds available VRAM budget"
        );
        let mut session = Self {
            device: device.clone(),
            allocations: vec![],
            input: 0,
            input_shape: graph.input_shape.clone(),
            error: 0,
            steps: vec![],
            outputs: vec![],
        };
        // On partial failure, release the context guard before dropping Session
        // (whose destructor takes the same device lock).
        let initialized = (|| -> Result<()> {
            let mut allocate = |data: Option<&[u32]>, bytes: usize| -> Result<Ptr> {
                let p = guard.allocate(bytes)?;
                session.allocations.push(p);
                if let Some(data) = data {
                    guard.upload(p, data)?;
                }
                Ok(p)
            };
            let slots = layout
                .slots
                .iter()
                .map(|&len| allocate(None, len * 4))
                .collect::<Result<Vec<_>>>()?;
            session.error = allocate(Some(&[0]), 4)?;
            let metadata_pointer = allocate(None, metadata_bytes)?;
            let mut metadata = Vec::with_capacity(metadata_words);
            let pointer = |v: usize| -> Result<Ptr> {
                Ok(slots[layout.values[v].context("CUDA missing value allocation")?])
            };
            session.input = pointer(graph.input)?;
            for (&value, words) in &graph.constants {
                if layout.values[value].is_some() {
                    guard.upload(pointer(value)?, words)?;
                }
            }
            for step in &graph.steps {
                let params = metadata_pointer + (metadata.len() * 4) as u64;
                metadata.extend_from_slice(&step.params);
                metadata.resize(metadata.len().next_multiple_of(2), 0);
                let table_start = metadata.len();
                let table_pointer = metadata_pointer + (table_start * 4) as u64;
                metadata.resize(table_start + 32, 0);
                let table = &mut metadata[table_start..];
                for (i, &v) in step.inputs.iter().enumerate() {
                    let p = pointer(v)?;
                    table[i * 2] = p as u32;
                    table[i * 2 + 1] = (p >> 32) as u32;
                }
                table[30] = session.error as u32;
                table[31] = (session.error >> 32) as u32;
                session.steps.push(Dispatch {
                    kernel: step.kernel,
                    blocks: step.blocks,
                    pointers: [table_pointer, 0, pointer(step.output)?, params],
                    len: graph.lengths[step.output] as u32,
                });
            }
            ensure!(
                metadata.len() == metadata_words,
                "CUDA metadata packing mismatch"
            );
            guard.upload(metadata_pointer, &metadata)?;
            session.outputs = graph
                .outputs
                .iter()
                .map(|(v, s)| Ok((pointer(*v)?, s.clone())))
                .collect::<Result<_>>()?;
            guard.finish_uploads()?;
            Ok(())
        })();
        drop(guard);
        initialized?;
        Ok(session)
    }
    fn run(&self, inputs: &TVec<TValue>) -> Result<TVec<TValue>> {
        ensure!(
            inputs.len() == 1 && inputs[0].shape() == self.input_shape,
            "CUDA input shape changed"
        );
        let view = inputs[0].to_plain_array_view::<f32>()?;
        let data = view.as_slice().context("noncontiguous CUDA input")?;
        ensure!(data.iter().all(|v| v.is_finite()), "nonfinite CUDA input");
        let guard = self.device.enter()?;
        guard.upload(self.error, &[0])?;
        // The input is uploaded once; every intermediate remains device-resident.
        // to_bits avoids alignment/aliasing assumptions at the FFI boundary.
        guard.upload(
            self.input,
            &data.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
        )?;
        guard.finish_uploads()?;
        for step in &self.steps {
            guard.launch(step.kernel, step.blocks, step.pointers, step.len)?;
        }
        let mut error = [0.];
        guard.download(self.error, &mut error)?;
        ensure!(
            error[0] == 0.,
            "CUDA kernel rejected an invalid index or operator"
        );
        let mut outputs = tvec![];
        for (pointer, shape) in &self.outputs {
            let mut output = vec![0.; shape.iter().product()];
            guard.download(*pointer, &mut output)?;
            ensure!(
                output.iter().all(|v| v.is_finite()),
                "nonfinite CUDA output"
            );
            outputs.push(Tensor::from_shape(shape, &output)?.into());
        }
        Ok(outputs)
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        if let Ok(guard) = self.device.enter() {
            let _ = guard.synchronize();
            for pointer in self.allocations.drain(..) {
                guard.free(pointer);
            }
        }
    }
}
