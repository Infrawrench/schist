//! Reuse immutable tile uploads across frames, without scanning pixel bytes.
use super::*;
use std::sync::Weak;

const MAX_BYTES: usize = 64 << 20;

// Weak references keep allocation identities unique without strongly owning
// the tiles. Arc::make_mut detaches even when only a Weak observes the old
// allocation, so an in-place edit cannot leave a matching key with stale data.
pub(super) enum Key {
    Pixels(Option<ColorMode>, Vec<(Weak<TileBuf>, u32)>),
    Masks(Vec<Weak<[u8; TILE_PIXELS]>>),
    Display(Vec<Weak<Vec<u8>>>),
}

impl PartialEq for Key {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Pixels(mode, tiles), Self::Pixels(other_mode, other_tiles)) => {
                mode == other_mode
                    && tiles.len() == other_tiles.len()
                    && tiles
                        .iter()
                        .zip(other_tiles)
                        .all(|((a, fmt), (b, other_fmt))| fmt == other_fmt && Weak::ptr_eq(a, b))
            }
            (Self::Masks(a), Self::Masks(b)) => same_tiles(a, b),
            (Self::Display(a), Self::Display(b)) => same_tiles(a, b),
            _ => false,
        }
    }
}

fn same_tiles<T>(a: &[Weak<T>], b: &[Weak<T>]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| Weak::ptr_eq(a, b))
}

#[derive(Default)]
pub(super) struct TileUploads {
    // Least recently used first. Chunk and viewport uploads share one budget.
    entries: std::collections::VecDeque<Entry>,
    bytes: usize,
    uploads: u64,
    hits: u64,
}

struct Entry {
    key: Key,
    buffer: wgpu::Buffer,
    bytes: usize,
}

/// The byte count covers retained GPU buffers; counters are cumulative.
#[derive(Debug, Default, Clone, Copy)]
pub struct TileUploadStats {
    pub bytes: usize,
    pub uploads: u64,
    pub hits: u64,
}

impl TileUploads {
    fn trim(&mut self, limit: usize) {
        while self.bytes > limit {
            let entry = self.entries.pop_front().unwrap();
            self.bytes -= entry.bytes;
        }
    }

    pub(super) fn upload(
        &mut self,
        device: &wgpu::Device,
        key: Key,
        bytes: usize,
        pack: impl FnOnce() -> Vec<u32>,
    ) -> wgpu::Buffer {
        if let Some(index) = self.entries.iter().position(|entry| entry.key == key) {
            let entry = self.entries.remove(index).unwrap();
            let buffer = entry.buffer.clone();
            self.entries.push_back(entry);
            self.hits += 1;
            return buffer;
        }
        // Make room before allocating the new upload. An oversized,
        // uncached batch also releases old uploads to reduce peak memory.
        self.trim(MAX_BYTES.saturating_sub(bytes));
        let words = pack();
        debug_assert_eq!(words.len() * 4, bytes);
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("cached-tiles"),
            contents: cast_u32s(if words.is_empty() { &[0] } else { &words }),
            usage: wgpu::BufferUsages::STORAGE,
        });
        self.uploads += 1;
        if (TILE_PIXELS..=MAX_BYTES).contains(&bytes) {
            self.bytes += bytes;
            self.entries.push_back(Entry {
                key,
                buffer: buffer.clone(),
                bytes,
            });
        }
        buffer
    }
}

impl GpuContext {
    /// Retained tile upload memory and cumulative transfer/cache-hit counts.
    pub fn tile_upload_stats(&self) -> TileUploadStats {
        let cache = self.tile_uploads.lock();
        TileUploadStats {
            bytes: cache.bytes,
            uploads: cache.uploads,
            hits: cache.hits,
        }
    }

    /// Release retained tile uploads, e.g. under memory pressure.
    pub fn clear_tile_uploads(&self) {
        self.tile_uploads.lock().trim(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn identity_changes_on_edit_without_retaining_pixels() {
        let mut tile = Arc::new(TileBuf::new(schist_color::Depth::Eight));
        let key = Key::Pixels(None, vec![(Arc::downgrade(&tile), 0)]);
        assert!(key == Key::Pixels(None, vec![(Arc::downgrade(&tile), 0)]));
        assert_eq!(Arc::strong_count(&tile), 1);
        Arc::make_mut(&mut tile).set(0, schist_color::Rgba::new(1.0, 0.0, 0.0, 1.0));
        assert!(key != Key::Pixels(None, vec![(Arc::downgrade(&tile), 0)]));
    }

    #[test]
    fn format_mode_and_tile_order_are_part_of_identity() {
        let a = Arc::new(TileBuf::new(schist_color::Depth::Eight));
        let b = Arc::new(TileBuf::new(schist_color::Depth::Eight));
        let key = Key::Pixels(None, vec![(Arc::downgrade(&a), 0), (Arc::downgrade(&b), 1)]);
        for other in [
            Key::Pixels(None, vec![(Arc::downgrade(&a), 1), (Arc::downgrade(&b), 1)]),
            Key::Pixels(
                Some(ColorMode::Cmyk),
                vec![(Arc::downgrade(&a), 0), (Arc::downgrade(&b), 1)],
            ),
            Key::Pixels(None, vec![(Arc::downgrade(&b), 1), (Arc::downgrade(&a), 0)]),
            Key::Pixels(None, vec![(Arc::downgrade(&a), 0)]),
        ] {
            assert!(key != other);
        }
    }
}
