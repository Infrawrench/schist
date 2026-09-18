fn effect(pos: vec2<i32>) -> vec4<f32> {
    let mode = u32(args[0]);
    let p = read_pixel(pos);
    let q = auxiliary_pixel(pos);
    let xy = vec2<f32>(pos);
    var color = p.rgb;
    if mode == 0u {
        let ink = min(q.r * (2.0 + args[1]) * 2.5, 1.0);
        let desired = select(0.0, 1.0, args[2] >= 0.5);
        let amount = max(abs(args[2] - 0.5) * 2.0, 0.15);
        color += (desired - color) * ink * amount;
    } else if mode == 1u || mode == 2u {
        let unit = 0.707 / sqrt(0.707 * 0.707 + 0.707 * 0.707);
        let up = stroke(pos, i32(args[1]), vec2<f32>(unit, -unit));
        let down = stroke(pos, i32(args[1]), vec2<f32>(unit, unit));
        if mode == 1u {
            let chosen = select(down, up, p.r > args[2]);
            return scalar_plane(chosen + (p.r - chosen) * args[3]);
        }
        return scalar_plane(p.r + (min(up, down) - p.r) * args[2]);
    } else if mode == 3u {
        return tint_tone(q, p.r + (luminance(q) - p.r) * args[1], args[2]);
    } else if mode == 4u {
        let l = q.r;
        var desired = 1.0;
        var amount = (l - args[1]) / max(1.0 - args[1], 0.001) * args[3];
        if l < args[1] {
            desired = 0.0;
            amount = (args[1] - l) / max(args[1], 0.001) * args[2];
        }
        let smeared = color + (l - luminance(p));
        color = smeared + (desired - smeared) * clamp(amount, 0.0, 1.0);
    } else if mode == 5u {
        let ink = min(q.r * 3.0, 1.0);
        let darkened = color * (1.0 - ink * args[1] * 1.6);
        color = darkened + (1.0 - darkened) * (1.0 - ink) * args[2] * luminance(p) * 0.4;
    } else if mode == 6u {
        let jitter = vec2<f32>(value_noise(xy * vec2<f32>(1.7, 1.3), 6151u), value_noise(xy * vec2<f32>(1.1, 1.9), 7919u)) - 0.5;
        return sample_plain(xy + jitter * args[1] * 2.0);
    } else if mode == 7u {
        let along = (value_noise(xy * 0.9, 3571u) - 0.5) * args[1];
        let across = (value_noise(xy * vec2<f32>(1.6, 1.2), 2963u) - 0.5) * args[2];
        let at = vec2<f32>(xy.x + args[3] * along - args[4] * across * 0.4, xy.y + args[4] * along + args[3] * across * 0.4);
        return sample_plain(at);
    } else if mode == 8u {
        let hard = clamp((p.r - 0.5) * (1.0 + args[2] * 1.5) + 0.5, 0.0, 1.0);
        return tint_tone(q, hard * (1.0 - (1.0 - hard) * args[1]), 0.5);
    }
    return vec4<f32>(clamp(color, vec3<f32>(0.0), vec3<f32>(1.0)), p.a);
}
