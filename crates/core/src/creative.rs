//! Lifecycle of editable blend/3D sources when ordinary layer tools touch them.
use crate::{Affine, Layer, RawBlock};
pub fn has_source(layer: &Layer) -> bool {
    layer
        .extras
        .iter()
        .any(|b| b.key == crate::vector_blend::BLOCK || b.key == crate::model3d::BLOCK)
}
pub fn without_sources(blocks: &[RawBlock]) -> Vec<RawBlock> {
    blocks
        .iter()
        .filter(|b| b.key != crate::vector_blend::BLOCK && b.key != crate::model3d::BLOCK)
        .cloned()
        .collect()
}
pub fn without_live_mask(blocks: &[RawBlock]) -> Vec<RawBlock> {
    blocks
        .iter()
        .filter(|b| b.key != crate::live_mask::BLOCK)
        .cloned()
        .collect()
}

pub fn transformed_extras(layer: &Layer, matrix: &Affine) -> Vec<RawBlock> {
    transformed_blocks(&layer.extras, matrix)
}
pub fn transformed_blocks(blocks: &[RawBlock], matrix: &Affine) -> Vec<RawBlock> {
    let mut blocks = blocks.to_vec();
    if let Some(mut blend) = crate::vector_blend::VectorBlend::from_blocks(&blocks) {
        let paths = [&mut blend.start.path, &mut blend.end.path]
            .into_iter()
            .chain(blend.spine.iter_mut())
            .chain(blend.rail.iter_mut());
        for path in paths {
            for sub in &mut path.subpaths {
                for a in &mut sub.anchors {
                    let old = a.point;
                    let new = matrix.apply(old.0, old.1);
                    for h in [&mut a.handle_in, &mut a.handle_out] {
                        let p = matrix.apply(old.0 + h.0, old.1 + h.1);
                        *h = (p.0 - new.0, p.1 - new.1);
                    }
                    a.point = new;
                }
            }
        }
        blocks = blend.replace_blocks(&blocks);
    }
    if let Some(mut model) = crate::model3d::Model3d::from_blocks(&blocks) {
        model.placement.transform = matrix.then(&model.placement.transform);
        blocks = model.replace_blocks(&blocks);
    }
    blocks
}

pub fn translated_extras(layer: &Layer, dx: i32, dy: i32) -> Vec<RawBlock> {
    let mut transformed = layer.clone();
    transformed.extras = transformed_extras(layer, &Affine::translate(dx as f32, dy as f32));
    if layer.mask.as_ref().is_some_and(|m| m.linked) {
        if let Some(mut mask) = crate::live_mask::LiveMask::from_layer(&transformed) {
            mask.translate(dx, dy);
            transformed.extras = mask.blocks(&transformed);
        }
    }
    transformed.extras
}
