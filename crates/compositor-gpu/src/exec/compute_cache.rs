//! Bounded, exact-content caching for immutable compute inputs and weights.
use super::*;
use std::hash::Hasher;

const MAX_BYTES: usize = 64 << 20;
const MIN_BYTES: usize = 1024;

#[derive(Default)]
pub(super) struct InputCache {
    entries: Vec<Entry>,
    bytes: usize,
    clock: u64,
    uploads: u64,
    hits: u64,
}

struct Entry {
    hash: u64,
    data: Vec<u8>,
    buffer: wgpu::Buffer,
    used: u64,
}

/// Counters include immutable primary inputs, parameters and model weights.
#[derive(Clone, Copy, Debug, Default)]
pub struct ComputeCacheStats {
    pub bytes: usize,
    pub uploads: u64,
    pub hits: u64,
}

impl InputCache {
    fn trim(&mut self, limit: usize) {
        while self.bytes > limit {
            let oldest = self
                .entries
                .iter()
                .enumerate()
                .min_by_key(|(_, entry)| entry.used)
                .map(|(i, _)| i)
                .unwrap();
            self.bytes -= self.entries.swap_remove(oldest).data.len();
        }
    }

    pub(super) fn upload(
        &mut self,
        device: &wgpu::Device,
        data: &[f32],
        allowance: usize,
    ) -> wgpu::Buffer {
        let data = crate::cast_f32s(if data.is_empty() { &[0.0] } else { data });
        let limit = allowance.min(MAX_BYTES);
        self.trim(limit);
        self.clock = self.clock.wrapping_add(1);
        let cacheable = data.len() >= MIN_BYTES && data.len() <= limit;
        let mut hasher = rustc_hash::FxHasher::default();
        if cacheable {
            hasher.write(data);
        }
        let hash = hasher.finish();
        if cacheable {
            // A hash only narrows the search: equality of every byte prevents
            // collisions, changed weights, or reused allocations from going stale.
            if let Some(entry) = self
                .entries
                .iter_mut()
                .find(|entry| entry.hash == hash && entry.data == data)
            {
                entry.used = self.clock;
                self.hits += 1;
                return entry.buffer.clone();
            }
        }
        self.uploads += 1;
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("compute-input"),
            contents: data,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        });
        if cacheable {
            self.trim(limit - data.len());
            self.bytes += data.len();
            self.entries.push(Entry {
                hash,
                data: data.to_vec(),
                buffer: buffer.clone(),
                used: self.clock,
            });
        }
        buffer
    }
}

impl GpuContext {
    pub fn compute_cache_stats(&self) -> ComputeCacheStats {
        let cache = self.compute_inputs.lock();
        ComputeCacheStats {
            bytes: cache.bytes,
            uploads: cache.uploads,
            hits: cache.hits,
        }
    }

    /// Release retained uploads, for document close and explicit memory pressure.
    pub fn clear_compute_cache(&self) {
        self.compute_inputs.lock().trim(0);
    }
}
