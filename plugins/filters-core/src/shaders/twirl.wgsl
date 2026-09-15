// args: CPU-prepared (dx, dy) pairs in full-image row-major order.
fn effect(pos: vec2<i32>) -> vec4<f32> {
    let i = (u32(pos.y) * image.width + u32(pos.x)) * 2u;
    let delta = vec2<f32>(args[i], args[i + 1u]);
    return straight(sample_premul_offset(pos, delta));
}
