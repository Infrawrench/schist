//! Schist's frame animation, in a private document-level block.
//!
//! `ScAn` is a Schist key in the document's additional layer information,
//! like `ScIn`: other readers skip a block they do not know. The payload
//! is JSON. Layers are referred to by their position in the depth-first,
//! bottom-up walk of the layer tree, which is the order the writer emits
//! them in and the reader rebuilds them in, with the layer's name kept
//! alongside as a check: a file whose layers were reordered or renamed
//! elsewhere falls back to matching unique names, and a state that
//! matches nothing is dropped rather than applied to the wrong layer.
//!
//! The current frame is written as the layers themselves -- the visibility
//! and opacity in each layer record -- so a reader that ignores this block
//! still opens the file showing the frame that was selected.
//!
//! Photoshop's own animation data is not interpreted. It lives in image
//! resources and per-layer metadata blocks that are preserved verbatim
//! like every other block Schist does not understand.

use schist_core::animation::{Frame, FrameLayerState, LoopCount, Timeline, MAX_DELAY_MS};
use schist_core::{Document, LayerId, RawBlock};
use serde::{Deserialize, Serialize};

pub const KEY: [u8; 4] = *b"ScAn";
const VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct Stored {
    version: u32,
    current: usize,
    /// Total plays; zero is forever, as in APNG and WebP.
    plays: u32,
    new_layers_visible: bool,
    /// Layer names, in tree walk order.
    layers: Vec<String>,
    frames: Vec<StoredFrame>,
}

#[derive(Serialize, Deserialize)]
struct StoredFrame {
    delay_ms: u32,
    /// (layer index, visible, opacity, x offset, y offset).
    states: Vec<(usize, bool, f32, i32, i32)>,
}

/// The block for `doc`'s timeline, or `None` without one.
pub fn block(doc: &Document) -> Option<RawBlock> {
    let timeline = doc.timeline.as_ref()?.synced(&doc.tree);
    let layers: Vec<(LayerId, String)> = doc.tree.iter().map(|l| (l.id, l.name.clone())).collect();
    let index = |id: LayerId| layers.iter().position(|(l, _)| *l == id);
    let stored = Stored {
        version: VERSION,
        current: timeline.current,
        plays: timeline.loop_count.plays().unwrap_or(0),
        new_layers_visible: timeline.new_layers_visible,
        layers: layers.iter().map(|(_, n)| n.clone()).collect(),
        frames: timeline
            .frames
            .iter()
            .map(|f| StoredFrame {
                delay_ms: f.delay_ms,
                states: f
                    .states
                    .iter()
                    .filter_map(|s| {
                        Some((
                            index(s.layer)?,
                            s.visible,
                            s.opacity,
                            s.offset.0,
                            s.offset.1,
                        ))
                    })
                    .collect(),
            })
            .collect(),
    };
    Some(RawBlock {
        key: KEY,
        data: serde_json::to_vec(&stored).ok()?,
    })
}

/// Restore the timeline from a document's `ScAn` block, once its layers
/// are read. The block is consumed; one that cannot be understood (a newer
/// version, say) stays preserved verbatim instead.
pub fn read(doc: &mut Document) {
    let Some(at) = doc.preserved_layer_info.iter().position(|b| b.key == KEY) else {
        return;
    };
    let Some(timeline) = parse(doc, &doc.preserved_layer_info[at].data) else {
        log::warn!("ignoring an unreadable ScAn animation block");
        return;
    };
    doc.preserved_layer_info.remove(at);
    doc.timeline = Some(timeline);
    schist_core::animation::apply_offsets(doc);
}

fn parse(doc: &Document, data: &[u8]) -> Option<Timeline> {
    let stored: Stored = serde_json::from_slice(data).ok()?;
    if stored.version != VERSION || stored.frames.is_empty() {
        return None;
    }
    let layers: Vec<(LayerId, &str)> = doc.tree.iter().map(|l| (l.id, l.name.as_str())).collect();
    // Index → live layer, checked by name, falling back to a unique name.
    let resolve = |index: usize| -> Option<LayerId> {
        let name = stored.layers.get(index)?;
        if let Some((id, live)) = layers.get(index) {
            if live == name {
                return Some(*id);
            }
        }
        let mut matches = layers.iter().filter(|(_, n)| n == name);
        match (matches.next(), matches.next()) {
            (Some((id, _)), None) => Some(*id),
            _ => None,
        }
    };
    let ids: Vec<Option<LayerId>> = (0..stored.layers.len()).map(resolve).collect();
    let frames = stored
        .frames
        .iter()
        .take(schist_core::animation::MAX_FRAMES)
        .map(|f| Frame {
            delay_ms: f.delay_ms.min(MAX_DELAY_MS),
            states: f
                .states
                .iter()
                .filter_map(|&(index, visible, opacity, dx, dy)| {
                    Some(FrameLayerState {
                        layer: (*ids.get(index)?)?,
                        visible,
                        opacity: if opacity.is_finite() {
                            opacity.clamp(0.0, 1.0)
                        } else {
                            1.0
                        },
                        offset: (dx, dy),
                    })
                })
                .collect(),
        })
        .collect::<Vec<_>>();
    Some(Timeline {
        current: stored.current.min(frames.len() - 1),
        frames,
        loop_count: match stored.plays {
            0 => LoopCount::Forever,
            n => LoopCount::Times(n),
        },
        new_layers_visible: stored.new_layers_visible,
    })
}
