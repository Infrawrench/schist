// args: one horizontal offset per full-image row, then one vertical per column.
fn effect(pos: vec2<i32>) -> vec4<f32> {
    let delta = vec2<f32>(args[u32(pos.y)], args[image.height + u32(pos.x)]);
    return straight(sample_premul_offset(pos, delta));
}
