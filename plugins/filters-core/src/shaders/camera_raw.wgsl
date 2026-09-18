fn band(v: f32, center: f32, radius: f32) -> f32 {
    let t = clamp((v - center) / radius, -1.0, 1.0);
    let s = 1.0 - t * t;
    return s * s;
}

fn effect(pos: vec2<i32>) -> vec4<f32> {
    let mode = u32(args[0]);
    let p = read_pixel(pos);
    let q = auxiliary_pixel(pos);
    var color = p.rgb;
    if mode == 0u {
        if args[1] != 0.0 || args[2] != 0.0 {
            color.r = clamp(color.r * (1.0 + args[1] * 0.35), 0.0, 1.0);
            color.b = clamp(color.b * (1.0 - args[1] * 0.35), 0.0, 1.0);
            color.g = clamp(color.g * (1.0 - args[2] * 0.25), 0.0, 1.0);
            color.r = clamp(color.r * (1.0 + args[2] * 0.12), 0.0, 1.0);
            color.b = clamp(color.b * (1.0 + args[2] * 0.12), 0.0, 1.0);
        }
        color = clamp(color * args[3], vec3<f32>(0.0), vec3<f32>(1.0));
        let l = luminance(vec4<f32>(color, p.a));
        var gain = 0.0;
        if args[5] != 0.0 {
            gain += args[5] * 0.5 * band(l, 0.8, 0.45);
        }
        if args[6] != 0.0 {
            gain += args[6] * 0.5 * band(l, 0.2, 0.45);
        }
        if args[7] != 0.0 {
            gain += args[7] * 0.35 * band(l, 1.0, 0.4);
        }
        if args[8] != 0.0 {
            gain += args[8] * 0.35 * band(l, 0.0, 0.4);
        }
        if gain != 0.0 {
            color = clamp(color + gain * max(1.0 - color, vec3<f32>(0.05)), vec3<f32>(0.0), vec3<f32>(1.0));
        }
        if args[4] != 0.0 {
            color = clamp((color - 0.5) * (1.0 + args[4]) + 0.5, vec3<f32>(0.0), vec3<f32>(1.0));
        }
    } else if mode == 1u {
        for (var c = 0u; c < 3u; c++) {
            color[c] = clamp(color[c] + (color[c] - q[c]) * args[1] * 1.5 * band(q[c], 0.5, 0.75), 0.0, 1.0);
        }
    } else if mode == 2u {
        let l = luminance(p);
        let sat = max(color.r, max(color.g, color.b)) - min(color.r, min(color.g, color.b));
        let amount = args[2] + args[1] * (1.0 - sat);
        color = clamp(l + (color - l) * (1.0 + amount), vec3<f32>(0.0), vec3<f32>(1.0));
    } else if mode == 3u {
        color += (q.rgb - color) * args[1];
    } else if mode == 4u {
        color = clamp(color + (color - q.rgb) * args[1], vec3<f32>(0.0), vec3<f32>(1.0));
    } else if mode == 5u {
        let center = vec2<f32>(f32(image.width), f32(image.height)) / 2.0;
        let d = length(vec2<f32>(pos) + 0.5 - center) / max(length(center), 1.0);
        let falloff = clamp(d * d * d, 0.0, 1.0);
        color = clamp(color * (1.0 - args[1] * falloff), vec3<f32>(0.0), vec3<f32>(1.0));
    }
    return vec4<f32>(color, p.a);
}
