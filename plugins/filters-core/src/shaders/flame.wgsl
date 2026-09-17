fn effect(pos: vec2<i32>) -> vec4<f32> {
    let p = read_pixel(pos);
    let count = u32(args[0]);
    let height = args[1];
    let width = args[2];
    let lean = args[3];
    let turbulence = args[4];
    let opacity = args[5];
    let seed = u32(args[6]);
    if height <= 0.0 || width <= 0.0 || opacity <= 0.0 {
        return p;
    }
    var heat = 0.0;
    for (var f = 0u; f < count; f++) {
        let at = 7u + f * 4u;
        let delta = vec2<f32>(pos) - vec2<f32>(args[at], args[at + 1u]);
        let normal = vec2<f32>(args[at + 2u], args[at + 3u]);
        let up = delta.x * normal.x + delta.y * normal.y;
        let rise = up / height;
        if rise < 0.0 || rise > 1.0 {
            continue;
        }
        let across = delta.x * -normal.y + delta.y * normal.x + lean * up;
        let wander = (fbm(vec2<f32>(up / (18.0 + 30.0 * (1.0 - turbulence)), f32(f) * 9.0), seed, 3u) - 0.5) * turbulence * width * 3.0 * rise;
        let taper = pow(1.0 - rise, 0.65) * (1.0 - max(rise - 0.05, 0.0) * 0.35);
        let reach = max(width * taper, 0.5);
        let d = abs((across - wander) / reach);
        if d < 1.0 {
            let tongue = fbm(vec2<f32>((across - wander) / 9.0, (up - rise * 60.0) / 7.0), seed ^ 0x51edu, 3u);
            let body = (1.0 - d * d) * (1.0 - rise * 0.85);
            heat = max(heat, max(body * (0.55 + tongue * 0.9), 0.0));
        }
    }
    if heat <= 0.01 {
        return p;
    }
    let t = min(heat, 1.0);
    let fire = vec3<f32>(min(t * 3.0, 1.0), clamp(t * 2.0 - 0.35, 0.0, 1.0), clamp(t * 3.2 - 2.1, 0.0, 1.0));
    let cover = min(t * 1.6, 1.0) * opacity;
    return vec4<f32>(min(p.rgb + fire * cover, vec3<f32>(1.0)), max(p.a, cover));
}
