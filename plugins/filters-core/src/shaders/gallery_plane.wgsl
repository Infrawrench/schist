fn effect(pos: vec2<i32>) -> vec4<f32> {
    let mode = u32(args[0]);
    let here = read_pixel(pos);
    if mode == 0u {
        return scalar_plane(luminance(here));
    }
    if mode == 1u {
        return scalar_plane(min(length(tone_gradient(pos)), 1.0));
    }
    if mode == 2u {
        return vec4<f32>(tone_gradient(pos), 0.0, 1.0);
    }
    if mode == 3u {
        return scalar_plane(stroke(pos, i32(args[1]), vec2<f32>(args[2], args[3])));
    }
    let r = i32(args[1]);
    let tolerance = args[2];
    var acc = vec3<f32>(0.0);
    var total = 0.0;
    for (var dy = -r; dy <= r; dy++) {
        for (var dx = -r; dx <= r; dx++) {
            if dx * dx + dy * dy > r * r {
                continue;
            }
            let p = read_pixel(pos + vec2<i32>(dx, dy));
            let delta = abs(p.rgb - here.rgb);
            let d = max(delta.r, max(delta.g, delta.b));
            if d > tolerance {
                continue;
            }
            let weight = 1.0 - d / tolerance;
            acc += p.rgb * weight;
            total += weight;
        }
    }
    return vec4<f32>(acc / max(total, 0.000001), here.a);
}
