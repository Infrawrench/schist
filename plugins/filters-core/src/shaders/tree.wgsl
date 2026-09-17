fn effect(pos: vec2<i32>) -> vec4<f32> {
    var result = read_pixel(pos);
    let bin = (u32(pos.y) / 16u * u32(args[0]) + u32(pos.x) / 16u) * 2u;
    let start = u32(aux[bin]);
    let count = u32(aux[bin + 1u]);
    for (var i = 0u; i < count; i++) {
        let j = u32(args[1]) + u32(aux[start + i]) * 9u;
        let delta = vec2<f32>(pos) - vec2<f32>(aux[j], aux[j + 1u]);
        let radius = aux[j + 2u];
        if length(delta) > radius {
            continue;
        }
        let across = (delta.x * aux[j + 4u] - delta.y * aux[j + 3u]) / max(radius, 0.001);
        let shade = 1.0 - clamp(across * aux[j + 5u], -0.8, 0.8) * 0.45;
        result = vec4<f32>(clamp(vec3<f32>(aux[j + 6u], aux[j + 7u], aux[j + 8u]) * shade, vec3<f32>(0.0), vec3<f32>(1.0)), max(result.a, 1.0));
    }
    return result;
}
