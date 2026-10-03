//! Frame animation: a timeline of frames over one layer stack.
//!
//! This is Photoshop's frame animation model. The document keeps one set
//! of layers; a frame records, per layer, whether it is visible, its
//! opacity and a position offset. Pixels are shared by every frame, so a
//! frame costs a few bytes per layer rather than a copy of the artwork.
//!
//! **The live layers are the current frame.** Whatever the Layers panel
//! shows -- visibility, opacity -- *is* the selected frame, so every
//! ordinary edit (toggling an eye, dragging an opacity slider, undoing
//! either) changes that frame without the edit knowing frames exist. The
//! stored copy of the current frame is therefore allowed to go stale, and
//! [`Timeline::synced`] reads the live layers back into it before anything
//! looks at the frames as a whole: switching frames, saving, rendering.
//!
//! Offsets are the exception. A layer has no persistent position property
//! to keep them in, so the timeline is their only home, and the layers'
//! transient `render_offset` mirrors the current frame's offsets so the
//! canvas shows them. Every timeline change goes through
//! [`EditOp::TimelineSet`](crate::history::EditOp::TimelineSet), whose
//! application re-derives `render_offset`, so undo and redo keep the two
//! in step.

use serde::{Deserialize, Serialize};

use crate::document::{Document, EditBuilder};
use crate::layer::{Layer, LayerId, LayerKind, LayerTree};

/// Photoshop's default frame delay is "no delay"; a tenth of a second is
/// what people actually want to see when they press play, and what most
/// GIF tools use.
pub const DEFAULT_DELAY_MS: u32 = 100;

/// The longest delay any of the export formats can carry: a GIF delay is
/// a 16-bit count of centiseconds.
pub const MAX_DELAY_MS: u32 = 655_350;

/// The longest the timeline may grow. Far beyond a hand-made animation;
/// it exists so a scripted caller cannot make every render loop unbounded.
pub const MAX_FRAMES: usize = 10_000;

/// One layer's state in one frame.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FrameLayerState {
    pub layer: LayerId,
    pub visible: bool,
    pub opacity: f32,
    /// Position offset in document pixels, relative to the layer's pixels.
    pub offset: (i32, i32),
}

/// A frame: how long it shows, and every layer's state while it does.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    pub delay_ms: u32,
    pub states: Vec<FrameLayerState>,
}

impl Frame {
    /// The state this frame recorded for `layer`, if it recorded one.
    pub fn state(&self, layer: LayerId) -> Option<&FrameLayerState> {
        self.states.iter().find(|s| s.layer == layer)
    }

    fn state_mut(&mut self, layer: LayerId) -> Option<&mut FrameLayerState> {
        self.states.iter_mut().find(|s| s.layer == layer)
    }

    /// Record every layer as it stands, keeping the offsets `previous`
    /// stored (the live layers have nowhere else to keep them).
    pub fn capture(tree: &LayerTree, delay_ms: u32, previous: Option<&Frame>) -> Frame {
        Frame {
            delay_ms,
            states: tree
                .iter()
                .map(|layer| FrameLayerState {
                    layer: layer.id,
                    visible: layer.visible,
                    opacity: layer.opacity,
                    offset: previous
                        .and_then(|f| f.state(layer.id))
                        .map_or((0, 0), |s| s.offset),
                })
                .collect(),
        }
    }
}

/// How many times an animation plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LoopCount {
    Forever,
    /// Total plays, at least one. `Times(1)` is Photoshop's "Once".
    Times(u32),
}

impl LoopCount {
    /// Total plays, or `None` for forever.
    pub fn plays(self) -> Option<u32> {
        match self {
            LoopCount::Forever => None,
            LoopCount::Times(n) => Some(n.max(1)),
        }
    }
}

/// The document's frame animation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Timeline {
    pub frames: Vec<Frame>,
    /// The selected frame, the one the live layers show.
    pub current: usize,
    pub loop_count: LoopCount,
    /// Photoshop's "New Layers Visible in All Frames": whether a layer
    /// created while one frame is selected also shows in the others.
    pub new_layers_visible: bool,
}

/// A layer's resolved state in a frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedState {
    pub visible: bool,
    pub opacity: f32,
    pub offset: (i32, i32),
}

impl Timeline {
    /// A one-frame timeline of the layers as they are.
    pub fn from_tree(tree: &LayerTree) -> Timeline {
        Timeline {
            frames: vec![Frame::capture(tree, DEFAULT_DELAY_MS, None)],
            current: 0,
            loop_count: LoopCount::Forever,
            new_layers_visible: true,
        }
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// The whole animation's running time, one play.
    pub fn duration_ms(&self) -> u64 {
        self.frames.iter().map(|f| f.delay_ms as u64).sum()
    }

    /// The current frame's stored offset for `layer`.
    pub fn current_offset(&self, layer: LayerId) -> (i32, i32) {
        self.frames
            .get(self.current)
            .and_then(|f| f.state(layer))
            .map_or((0, 0), |s| s.offset)
    }

    /// `layer`'s state in frame `index`.
    ///
    /// The current frame is the live layer. Any other frame uses what it
    /// recorded; a layer it has no record of -- one created since the
    /// frame was last synced -- follows the new-layer rule from the live
    /// layer, as [`Self::synced`] would materialise it.
    pub fn resolve(&self, index: usize, layer: &Layer) -> ResolvedState {
        let stored = self.frames.get(index).and_then(|f| f.state(layer.id));
        if index == self.current {
            return ResolvedState {
                visible: layer.visible,
                opacity: layer.opacity,
                offset: stored.map_or((0, 0), |s| s.offset),
            };
        }
        match stored {
            Some(s) => ResolvedState {
                visible: s.visible,
                opacity: s.opacity,
                offset: s.offset,
            },
            None => ResolvedState {
                visible: self.new_layers_visible && layer.visible,
                opacity: layer.opacity,
                offset: (0, 0),
            },
        }
    }

    /// This timeline with the current frame read back from the live
    /// layers, and every layer some frame has no record of given one by
    /// the new-layer rule. Records of layers that no longer exist are
    /// kept: undoing their deletion brings the layer back with its id.
    pub fn synced(&self, tree: &LayerTree) -> Timeline {
        let mut out = self.clone();
        if out.frames.is_empty() {
            out.frames
                .push(Frame::capture(tree, DEFAULT_DELAY_MS, None));
        }
        out.current = out.current.min(out.frames.len() - 1);
        let current = out.current;
        let keep: Vec<FrameLayerState> = out.frames[current]
            .states
            .iter()
            .filter(|s| tree.find(s.layer).is_none())
            .copied()
            .collect();
        let delay = out.frames[current].delay_ms;
        let mut captured = Frame::capture(tree, delay, Some(&out.frames[current]));
        captured.states.extend(keep);
        out.frames[current] = captured;
        for (index, frame) in out.frames.iter_mut().enumerate() {
            if index == current {
                continue;
            }
            for layer in tree.iter() {
                if frame.state(layer.id).is_none() {
                    frame.states.push(FrameLayerState {
                        layer: layer.id,
                        visible: self.new_layers_visible && layer.visible,
                        opacity: layer.opacity,
                        offset: (0, 0),
                    });
                }
            }
        }
        out
    }

    /// A copy of `tree` showing frame `index`: visibility and opacity set,
    /// offsets applied as render offsets. For rendering; the copy shares
    /// every tile with the original.
    pub fn frame_tree(&self, tree: &LayerTree, index: usize) -> LayerTree {
        let mut out = tree.clone();
        fn walk(timeline: &Timeline, index: usize, layers: &mut [Layer]) {
            for layer in layers {
                let state = timeline.resolve(index, layer);
                layer.visible = state.visible;
                layer.opacity = state.opacity;
                layer.render_offset = state.offset;
                if let LayerKind::Group(group) = &mut layer.kind {
                    walk(timeline, index, &mut group.children);
                }
            }
        }
        walk(self, index, &mut out.layers);
        out
    }

    /// A fingerprint of what frame `index` looks like: every layer's
    /// resolved state and the identity of its pixels. Equal keys mean an
    /// identical render, so thumbnails and playback images are cached on
    /// it rather than on the document revision, which every frame switch
    /// bumps.
    pub fn frame_key(&self, tree: &LayerTree, index: usize) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = rustc_hash::FxHasher::default();
        for layer in tree.iter() {
            let state = self.resolve(index, layer);
            layer.id.hash(&mut h);
            state.visible.hash(&mut h);
            state.opacity.to_bits().hash(&mut h);
            state.offset.hash(&mut h);
            layer.fill_opacity.to_bits().hash(&mut h);
            format!("{:?}", layer.blend).hash(&mut h);
            layer.clipping.hash(&mut h);
            match &layer.kind {
                LayerKind::Raster(r) => r.tiles.fingerprint().hash(&mut h),
                LayerKind::Adjustment(a) => {
                    a.raw.hash(&mut h);
                    a.params_json.hash(&mut h);
                }
                LayerKind::Group(g) => g.children.len().hash(&mut h),
            }
            if let Some(mask) = &layer.mask {
                mask.enabled.hash(&mut h);
                mask.default_value.hash(&mut h);
                for (coord, tile) in mask.tiles.iter() {
                    coord.hash(&mut h);
                    (std::sync::Arc::as_ptr(tile) as *const u8 as usize).hash(&mut h);
                }
            }
            if let Some(styled) = &layer.styled {
                styled.key.hash(&mut h);
            }
            format!("{:?}", layer.style).hash(&mut h);
        }
        h.finish()
    }
}

/// Set every layer's `render_offset` to the current frame's offset; zero
/// when there is no animation. What [`EditOp::TimelineSet`] calls after
/// replacing the timeline, so the canvas always shows the current frame.
///
/// [`EditOp::TimelineSet`]: crate::history::EditOp::TimelineSet
pub fn apply_offsets(doc: &mut Document) -> bool {
    let mut changed = false;
    let timeline = doc.timeline.take();
    fn walk(timeline: Option<&Timeline>, layers: &mut [Layer], changed: &mut bool) {
        for layer in layers {
            let offset = timeline.map_or((0, 0), |t| t.current_offset(layer.id));
            if layer.render_offset != offset {
                layer.render_offset = offset;
                *changed = true;
            }
            if let LayerKind::Group(group) = &mut layer.kind {
                walk(timeline, &mut group.children, changed);
            }
        }
    }
    walk(timeline.as_ref(), &mut doc.tree.layers, &mut changed);
    doc.timeline = timeline;
    changed
}

impl EditBuilder<'_> {
    /// Show the timeline's current frame on the live layers: set each
    /// layer's visibility and opacity from the frame, recorded as
    /// ordinary property changes so they undo with the timeline.
    pub fn apply_current_frame(&mut self) {
        let Some(timeline) = self.doc().timeline.clone() else {
            return;
        };
        let Some(frame) = timeline.frames.get(timeline.current) else {
            return;
        };
        let ids: Vec<LayerId> = self.doc().tree.iter().map(|l| l.id).collect();
        for id in ids {
            let Some(state) = frame.state(id).copied() else {
                continue;
            };
            self.change_props(id, |layer| {
                layer.visible = state.visible;
                layer.opacity = state.opacity;
            });
        }
    }
}

/// Why a frame operation did nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The document has no frame animation.
    NoAnimation,
    /// It already has one.
    AlreadyAnimated,
    /// The last frame cannot be deleted; delete the animation instead.
    LastFrame,
    /// The timeline is at [`MAX_FRAMES`].
    TooManyFrames,
    /// Nothing to make frames from.
    NoLayers,
    /// The frame index is out of range, or the change is a no-op.
    Nothing,
}

pub type FrameResult = Result<(), Refusal>;

/// Replace the timeline with `after` (already synced) and show its current
/// frame, as one undoable edit.
fn commit(doc: &mut Document, name: &str, after: Option<Timeline>) -> FrameResult {
    if doc.timeline == after {
        return Err(Refusal::Nothing);
    }
    let mut edit = doc.begin_edit(name);
    edit.set_timeline(after);
    edit.apply_current_frame();
    if edit.commit() {
        Ok(())
    } else {
        Err(Refusal::Nothing)
    }
}

/// The synced timeline, for an operation to modify and [`commit`].
fn synced(doc: &Document) -> Result<Timeline, Refusal> {
    doc.timeline
        .as_ref()
        .map(|t| t.synced(&doc.tree))
        .ok_or(Refusal::NoAnimation)
}

/// Start a frame animation whose one frame is the document as it stands.
pub fn create(doc: &mut Document, name: &str) -> FrameResult {
    if doc.timeline.is_some() {
        return Err(Refusal::AlreadyAnimated);
    }
    commit(doc, name, Some(Timeline::from_tree(&doc.tree)))
}

/// Remove the animation. The layers keep showing the frame that was
/// selected; offsets, which only frames can hold, go back to zero.
pub fn delete_animation(doc: &mut Document, name: &str) -> FrameResult {
    if doc.timeline.is_none() {
        return Err(Refusal::NoAnimation);
    }
    commit(doc, name, None)
}

/// Select frame `index`, showing it on the layers.
pub fn select(doc: &mut Document, index: usize, name: &str) -> FrameResult {
    let mut t = synced(doc)?;
    if index >= t.frames.len() || index == t.current {
        return Err(Refusal::Nothing);
    }
    t.current = index;
    commit(doc, name, Some(t))
}

/// Select the frame `step` places from the current one, wrapping around.
pub fn step(doc: &mut Document, step: isize, name: &str) -> FrameResult {
    let t = doc.timeline.as_ref().ok_or(Refusal::NoAnimation)?;
    let len = t.frames.len() as isize;
    let index = (t.current as isize + step).rem_euclid(len.max(1)) as usize;
    select(doc, index, name)
}

/// Photoshop's New Frame: a copy of the selected frame, inserted after it
/// and selected.
pub fn duplicate(doc: &mut Document, name: &str) -> FrameResult {
    let mut t = synced(doc)?;
    if t.frames.len() >= MAX_FRAMES {
        return Err(Refusal::TooManyFrames);
    }
    let copy = t.frames[t.current].clone();
    t.frames.insert(t.current + 1, copy);
    t.current += 1;
    commit(doc, name, Some(t))
}

/// Delete frame `index`. The selection moves to its neighbour.
pub fn delete(doc: &mut Document, index: usize, name: &str) -> FrameResult {
    let mut t = synced(doc)?;
    if index >= t.frames.len() {
        return Err(Refusal::Nothing);
    }
    if t.frames.len() == 1 {
        return Err(Refusal::LastFrame);
    }
    t.frames.remove(index);
    // Deleting the selected frame selects the one before it, as Photoshop
    // does; deleting an earlier one keeps the same frame selected.
    if index < t.current || (index == t.current && t.current > 0) {
        t.current -= 1;
    }
    commit(doc, name, Some(t))
}

/// Move frame `from` so it lands at index `to`.
pub fn reorder(doc: &mut Document, from: usize, to: usize, name: &str) -> FrameResult {
    let mut t = synced(doc)?;
    let len = t.frames.len();
    if from >= len || to >= len || from == to {
        return Err(Refusal::Nothing);
    }
    let selected = t.current;
    let frame = t.frames.remove(from);
    t.frames.insert(to, frame);
    // The selected frame keeps being selected wherever it went.
    t.current = if selected == from {
        to
    } else {
        let mut i = selected;
        if from < i {
            i -= 1;
        }
        if to <= i {
            i += 1;
        }
        i
    };
    commit(doc, name, Some(t))
}

/// Reverse the frame order, keeping the same frame selected.
pub fn reverse(doc: &mut Document, name: &str) -> FrameResult {
    let mut t = synced(doc)?;
    if t.frames.len() < 2 {
        return Err(Refusal::Nothing);
    }
    t.frames.reverse();
    t.current = t.frames.len() - 1 - t.current;
    commit(doc, name, Some(t))
}

/// Set one frame's delay, or every frame's when `index` is `None`.
pub fn set_delay(doc: &mut Document, index: Option<usize>, ms: u32, name: &str) -> FrameResult {
    let mut t = synced(doc)?;
    let ms = ms.min(MAX_DELAY_MS);
    match index {
        Some(i) => t.frames.get_mut(i).ok_or(Refusal::Nothing)?.delay_ms = ms,
        None => t.frames.iter_mut().for_each(|f| f.delay_ms = ms),
    }
    commit(doc, name, Some(t))
}

pub fn set_loop(doc: &mut Document, loop_count: LoopCount, name: &str) -> FrameResult {
    let mut t = synced(doc)?;
    t.loop_count = match loop_count {
        LoopCount::Times(n) => LoopCount::Times(n.clamp(1, u16::MAX as u32)),
        forever => forever,
    };
    commit(doc, name, Some(t))
}

pub fn set_new_layers_visible(doc: &mut Document, visible: bool, name: &str) -> FrameResult {
    let mut t = synced(doc)?;
    t.new_layers_visible = visible;
    commit(doc, name, Some(t))
}

/// Set `layer`'s offset in the selected frame.
pub fn set_offset(
    doc: &mut Document,
    layer: LayerId,
    offset: (i32, i32),
    name: &str,
) -> FrameResult {
    let mut t = synced(doc)?;
    let current = t.current;
    let state = t.frames[current].state_mut(layer).ok_or(Refusal::Nothing)?;
    state.offset = offset;
    commit(doc, name, Some(t))
}

/// Photoshop's Make Frames From Layers: one frame per top-level layer,
/// bottom to top, each showing only that layer. Layers inside groups keep
/// their own visibility, so a group shows as it does now. Replaces any
/// frames the document had; the loop setting survives.
pub fn make_frames_from_layers(doc: &mut Document, name: &str) -> FrameResult {
    let top: Vec<LayerId> = doc.tree.layers.iter().map(|l| l.id).collect();
    if top.is_empty() {
        return Err(Refusal::NoLayers);
    }
    let previous = doc.timeline.as_ref().map(|t| t.synced(&doc.tree));
    let base = Frame::capture(&doc.tree, DEFAULT_DELAY_MS, None);
    let frames = top
        .iter()
        .take(MAX_FRAMES)
        .map(|&shown| {
            let mut frame = base.clone();
            for state in &mut frame.states {
                if top.contains(&state.layer) {
                    state.visible = state.layer == shown;
                }
            }
            frame
        })
        .collect();
    let t = Timeline {
        frames,
        current: 0,
        loop_count: previous
            .as_ref()
            .map_or(LoopCount::Forever, |t| t.loop_count),
        new_layers_visible: previous.as_ref().is_none_or(|t| t.new_layers_visible),
    };
    commit(doc, name, Some(t))
}

/// Flatten Frames Into Layers, once the caller has rendered the frames.
///
/// `layers` holds one new raster layer per frame, in frame order; they go
/// on top of the stack, bottom to top, and each frame shows only its own.
/// The caller renders because the kernel has no compositor.
pub fn add_flattened_layers(doc: &mut Document, layers: Vec<Layer>, name: &str) -> FrameResult {
    let mut t = synced(doc)?;
    if layers.len() != t.frames.len() {
        return Err(Refusal::Nothing);
    }
    let ids: Vec<LayerId> = layers.iter().map(|l| l.id).collect();
    let mut edit = doc.begin_edit(name);
    for layer in layers {
        let top = edit.doc().tree.layers.len();
        edit.insert_layer(crate::layer::LayerPath(vec![top]), layer);
    }
    for (index, frame) in t.frames.iter_mut().enumerate() {
        for (i, &id) in ids.iter().enumerate() {
            frame.states.push(FrameLayerState {
                layer: id,
                visible: i == index,
                opacity: 1.0,
                offset: (0, 0),
            });
        }
    }
    edit.set_timeline(Some(t));
    edit.apply_current_frame();
    if edit.commit() {
        Ok(())
    } else {
        Err(Refusal::Nothing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use schist_color::Depth;

    fn doc3() -> (Document, [LayerId; 3]) {
        let mut doc = Document::new("anim", 16, 16, Depth::Eight);
        let a = doc.push_layer(Layer::new_raster("a"));
        let b = doc.push_layer(Layer::new_raster("b"));
        let c = doc.push_layer(Layer::new_raster("c"));
        (doc, [a, b, c])
    }

    fn visible(doc: &Document, id: LayerId) -> bool {
        doc.tree.find(id).unwrap().visible
    }

    #[test]
    fn frames_from_layers_show_one_layer_each_and_undo_restores_everything() {
        let (mut doc, [a, b, c]) = doc3();
        make_frames_from_layers(&mut doc, "make").unwrap();
        let t = doc.timeline.as_ref().unwrap();
        assert_eq!(t.frames.len(), 3);
        assert_eq!(t.current, 0);
        assert!(visible(&doc, a) && !visible(&doc, b) && !visible(&doc, c));
        select(&mut doc, 2, "select").unwrap();
        assert!(!visible(&doc, a) && !visible(&doc, b) && visible(&doc, c));
        doc.undo();
        assert_eq!(doc.timeline.as_ref().unwrap().current, 0);
        assert!(visible(&doc, a) && !visible(&doc, c));
        doc.undo();
        assert!(doc.timeline.is_none());
        assert!(visible(&doc, a) && visible(&doc, b) && visible(&doc, c));
        doc.redo();
        doc.redo();
        assert_eq!(doc.timeline.as_ref().unwrap().current, 2);
        assert!(visible(&doc, c) && !visible(&doc, a));
    }

    #[test]
    fn edits_on_the_live_layers_belong_to_the_selected_frame() {
        let (mut doc, [a, b, _]) = doc3();
        create(&mut doc, "create").unwrap();
        duplicate(&mut doc, "new frame").unwrap();
        assert_eq!(doc.timeline.as_ref().unwrap().current, 1);
        // Hide `a` on frame 2 the ordinary way.
        let mut edit = doc.begin_edit("hide");
        edit.change_props(a, |l| l.visible = false);
        edit.commit();
        select(&mut doc, 0, "select").unwrap();
        assert!(visible(&doc, a), "frame 1 never hid it");
        select(&mut doc, 1, "select").unwrap();
        assert!(!visible(&doc, a), "frame 2 remembered");
        assert!(visible(&doc, b));
        // Undo the two selects and the hide: frame 2 shows `a` again.
        doc.undo();
        doc.undo();
        doc.undo();
        assert_eq!(doc.timeline.as_ref().unwrap().current, 1);
        assert!(visible(&doc, a));
    }

    #[test]
    fn delete_reorder_reverse_and_delay_keep_the_selection_sensible() {
        let (mut doc, _) = doc3();
        make_frames_from_layers(&mut doc, "make").unwrap();
        set_delay(&mut doc, Some(1), 250, "delay").unwrap();
        set_delay(&mut doc, None, 40, "delay all").unwrap();
        assert!(doc
            .timeline
            .as_ref()
            .unwrap()
            .frames
            .iter()
            .all(|f| f.delay_ms == 40));
        doc.undo();
        assert_eq!(doc.timeline.as_ref().unwrap().frames[1].delay_ms, 250);
        select(&mut doc, 1, "select").unwrap();
        let shown = doc.timeline.as_ref().unwrap().frames[1].clone();
        reorder(&mut doc, 1, 2, "move").unwrap();
        let t = doc.timeline.as_ref().unwrap();
        assert_eq!(t.current, 2, "the selected frame moved with the drag");
        assert_eq!(t.frames[2].delay_ms, shown.delay_ms);
        reorder(&mut doc, 0, 2, "move").unwrap();
        assert_eq!(doc.timeline.as_ref().unwrap().current, 1);
        reverse(&mut doc, "reverse").unwrap();
        assert_eq!(doc.timeline.as_ref().unwrap().current, 1);
        delete(&mut doc, 1, "delete").unwrap();
        let t = doc.timeline.as_ref().unwrap();
        assert_eq!(t.frames.len(), 2);
        assert_eq!(t.current, 0);
        delete(&mut doc, 0, "delete").unwrap();
        assert_eq!(delete(&mut doc, 0, "delete"), Err(Refusal::LastFrame));
        set_loop(&mut doc, LoopCount::Times(3), "loop").unwrap();
        assert_eq!(doc.timeline.as_ref().unwrap().loop_count.plays(), Some(3));
    }

    #[test]
    fn offsets_follow_the_current_frame_through_undo() {
        let (mut doc, [a, _, _]) = doc3();
        create(&mut doc, "create").unwrap();
        duplicate(&mut doc, "new").unwrap();
        set_offset(&mut doc, a, (5, -3), "offset").unwrap();
        assert_eq!(doc.tree.find(a).unwrap().render_offset, (5, -3));
        select(&mut doc, 0, "select").unwrap();
        assert_eq!(doc.tree.find(a).unwrap().render_offset, (0, 0));
        doc.undo();
        assert_eq!(doc.tree.find(a).unwrap().render_offset, (5, -3));
        doc.undo();
        assert_eq!(doc.tree.find(a).unwrap().render_offset, (0, 0));
        let t = doc.timeline.as_ref().unwrap();
        let rendered = t.frame_tree(&doc.tree, 1);
        assert_eq!(rendered.find(a).unwrap().render_offset, (0, 0));
        doc.redo();
        let t = doc.timeline.as_ref().unwrap();
        assert_ne!(t.frame_key(&doc.tree, 0), t.frame_key(&doc.tree, 1));
        delete_animation(&mut doc, "delete").unwrap();
        assert_eq!(doc.tree.find(a).unwrap().render_offset, (0, 0));
    }

    #[test]
    fn new_layers_follow_the_visible_in_all_frames_rule() {
        for all in [true, false] {
            let (mut doc, _) = doc3();
            create(&mut doc, "create").unwrap();
            let _ = set_new_layers_visible(&mut doc, all, "rule");
            duplicate(&mut doc, "new").unwrap();
            let mut edit = doc.begin_edit("layer");
            let id = edit.insert_layer(crate::layer::LayerPath(vec![3]), Layer::new_raster("new"));
            edit.commit();
            select(&mut doc, 0, "select").unwrap();
            assert_eq!(visible(&doc, id), all, "rule {all}");
            select(&mut doc, 1, "select").unwrap();
            assert!(visible(&doc, id), "it was made visible on frame 2");
        }
    }

    #[test]
    fn identical_frames_share_a_key_and_others_do_not() {
        let (mut doc, _) = doc3();
        create(&mut doc, "create").unwrap();
        duplicate(&mut doc, "new").unwrap();
        let t = doc.timeline.as_ref().unwrap().synced(&doc.tree);
        assert_eq!(t.frame_key(&doc.tree, 0), t.frame_key(&doc.tree, 1));
        make_frames_from_layers(&mut doc, "make").unwrap();
        let t = doc.timeline.as_ref().unwrap();
        assert_ne!(t.frame_key(&doc.tree, 0), t.frame_key(&doc.tree, 1));
    }
}
