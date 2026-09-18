// args: angle in radians. Displacements remain on the GPU.
fn effect(pos: vec2<i32>) -> vec4<f32> {
    let center = vec2<f32>(f32(image.width), f32(image.height)) / 2.0;
    let delta = vec2<f32>(pos) + 0.5 - center;
    let radius_squared = dot(center, center);
    let distance_squared = dot(delta, delta);
    var offset = vec2<f32>(0.0);
    if distance_squared < radius_squared {
        let falloff = 1.0 - sqrt(distance_squared / radius_squared);
        // Match the CPU's powi(2) before applying the requested angle.
        let angle = args[0] * (falloff * falloff);
        let s = precise_sin_cos(angle).x;
        let half_sine = precise_sin_cos(angle * 0.5).x;
        // Avoid subtracting nearly equal numbers near the falloff boundary.
        let c = -2.0 * half_sine * half_sine;
        offset = vec2<f32>(delta.x * c - delta.y * s, delta.x * s + delta.y * c);
    }
    return straight(sample_premul_offset(pos, offset));
}
