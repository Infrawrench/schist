// args: normalized amount, size.
fn effect(pos: vec2<i32>) -> vec4<f32> {
    let p = vec2<f32>(pos) + vec2<f32>(0.5);
    let delta = sin(p.yx / args[1]) * args[0] * args[1] * 0.25;
    return straight(sample_premul_offset(pos, delta));
}
