fn effect(pos: vec2<i32>) -> vec4<f32> {
    let mode = u32(args[0]);
    let p = read_pixel(pos);
    let q = auxiliary_pixel(pos);
    let xy = vec2<f32>(pos);
    var color = p.rgb;
    if mode == 0u {
        return flat_color(p, args[1], args[2]);
    }
    if mode == 1u {
        let bristle = value_noise(xy * 0.6, 91u) - 0.5;
        color = flat_color(p, args[1], 0.05).rgb + bristle * 0.06 * args[2];
    } else if mode == 2u {
        let n = value_noise(xy, 4127u) - 0.5;
        let l = luminance(p);
        let weight = clamp(1.0 - abs(l - 0.35), 0.0, 1.0);
        color += n * args[1] * weight * 0.9;
        if l > args[2] {
            color += (1.0 - color) * (l - args[2]) * args[3];
        }
    } else if mode == 3u {
        let dab = value_noise(xy * 0.35, 613u) - 0.5;
        let stain = min(q.r * 2.0, 1.0);
        color = ((color - 0.5) * 1.35 + 0.5) * (1.0 - stain * 0.7) + dab * 0.08 * args[1];
    } else if mode == 4u {
        let phase = (xy.x + xy.y) / args[1];
        let hatch = abs(sin(phase * 3.141592653589793));
        let ink = ((1.0 - luminance(p)) * 1.4 + q.r * 2.0) * args[2];
        let drawn = select(1.0, 1.0 - min(ink, 1.0), hatch < ink);
        let tone = clamp(drawn * (0.75 + args[3] * 0.25), 0.0, 1.0);
        color = tint_tone(p, tone, 0.35).rgb * (1.0 - tone) + vec3<f32>(args[4], args[5], args[6]) * tone;
    } else if mode == 5u {
        let base = luminance(p) * 0.25;
        let glow = min(q.r * (1.0 + args[1] * 6.0), 1.0);
        color = base + vec3<f32>(args[2], args[3], args[4]) * glow;
    } else if mode == 6u {
        var direction = vec2<f32>(-q.g, q.r);
        let n = length(direction);
        if n < 0.0001 {
            direction = vec2<f32>(1.0, 0.0);
        } else {
            direction /= n;
        }
        var acc = vec3<f32>(0.0);
        var count = 0.0;
        for (var t = -i32(args[1]); t <= i32(args[1]); t++) {
            acc += sample_plain(xy + direction * f32(t)).rgb;
            count += 1.0;
        }
        let dabbed = acc / count;
        color = dabbed + (p.rgb - dabbed) * args[2];
    } else if mode == 8u {
        let slope = (-q.r - q.g) * (6.0 + args[2] * 24.0);
        let light = clamp(slope, 0.0, 1.0);
        let shine = light * light * args[1];
        let shadow = clamp(-slope, 0.0, 1.0) * args[1] * 0.4;
        color = color * (1.0 - shadow) + shine;
    } else if mode == 9u {
        let ink = min(q.r * (1.0 + args[2] * 8.0), 1.0);
        color = flat_color(p, args[1], 0.1).rgb * (1.0 - ink);
    } else if mode == 10u {
        let tex = surface(u32(args[1]), xy, max(args[2], 1.0), 17u);
        let tone = clamp(q.r * (1.0 + (tex - 0.5) * args[3]), 0.0, 1.0);
        let detailed = color / max(luminance(p), 0.0001) * tone;
        color = tone + (detailed - tone) * (0.4 + args[4] / 40.0);
    } else if mode == 11u {
        let l = luminance(p);
        let amount = clamp((1.0 - l) * args[2], 0.0, 1.0);
        var tone = l + (q.r - l) * amount;
        if l > args[1] {
            tone = min(tone + (l - args[1]), 1.0);
        }
        color *= tone / max(l, 0.0001);
    } else if mode == 12u {
        let blotch = fbm(xy / args[1] / 3.0, 733u, 3u);
        let hole = clamp((blotch - 0.5) * (2.0 + args[2] * 6.0) + 0.5, 0.0, 1.0);
        let dark = color * (0.45 + 0.55 * hole);
        color += (dark - color) * (0.4 + args[2] * 0.6);
    } else if mode == 13u {
        let tex = surface(u32(args[1]), xy, max(args[2], 1.0), 29u);
        let base = clamp(color * (1.0 + (tex - 0.5) * args[3]), vec3<f32>(0.0), vec3<f32>(1.0));
        color = base + (q.rgb - base) * (1.0 - args[4]) * 0.6;
    } else if mode == 14u {
        let paper = value_noise(xy * 0.9, 271u) - 0.5;
        let pool = min(q.r * 3.0, 1.0) * (0.35 + args[2]);
        let washed = color * (1.0 - pool);
        color = washed * (1.0 - (1.0 - washed) * args[2] * 0.5) + paper * 0.05 * args[1];
    }
    return vec4<f32>(clamp(color, vec3<f32>(0.0), vec3<f32>(1.0)), p.a);
}
