// args: angle in radians. Displacements remain on the GPU.
fn effect(pos: vec2<i32>) -> vec4<f32> {
    let center = vec2<f32>(f32(image.width), f32(image.height)) / 2.0;
    let delta = vec2<f32>(pos) + 0.5 - center;
    let radius = length(center);
    let d = length(delta);
    var offset = vec2<f32>(0.0);
    if d < radius {
        let falloff = 1.0 - d / radius;
        let angle = args[0] * falloff * falloff;
        let s = sin(angle);
        let c = cos(angle) - 1.0;
        offset = vec2<f32>(delta.x * c - delta.y * s, delta.x * s + delta.y * c);
    }
    return straight(sample_premul_offset(pos, offset));
}
