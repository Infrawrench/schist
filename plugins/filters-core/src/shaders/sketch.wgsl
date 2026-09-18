fn effect(pos: vec2<i32>) -> vec4<f32> {
    let mode = u32(args[0]);
    let p = read_pixel(pos);
    let q = auxiliary_pixel(pos);
    let xy = vec2<f32>(pos);
    var tone = p.r;
    if mode == 100u {
        let ink = vec3<f32>(args[1], args[2], args[3]);
        let paper = vec3<f32>(args[4], args[5], args[6]);
        return vec4<f32>(ink + (paper - ink) * clamp(tone, 0.0, 1.0), q.a);
    }
    if mode == 0u {
        let grad = tone_gradient(pos);
        tone = 0.5 + (grad.x * args[1] + grad.y * args[2]) * args[3];
    } else if mode == 1u {
        let unit = 0.707 / sqrt(0.707 * 0.707 + 0.707 * 0.707);
        if tone < 0.5 {
            let dark = stroke(pos, 4, vec2<f32>(unit, unit));
            let t = min((0.5 - tone) / 0.5 * (0.6 + args[1] * 2.0), 1.0) * args[3];
            tone = 0.5 - t * (0.5 + (0.5 - dark));
        } else {
            let light = stroke(pos, 4, vec2<f32>(unit, -unit));
            let t = min((tone - 0.5) / 0.5 * (0.6 + args[2] * 2.0), 1.0) * args[3];
            tone = 0.5 + t * (0.5 + (light - 0.5));
        }
    } else if mode == 2u {
        let ink = min(q.r * (2.0 + args[1] * 6.0), 1.0) * (0.5 + args[1] * 0.5) + (1.0 - tone) * args[2];
        tone = 1.0 - ink;
    } else if mode == 3u {
        tone = 0.5 + sin((tone - 0.5) * (2.0 + args[1]) * 3.141592653589793) * 0.5;
    } else if mode == 4u {
        let tex = surface(u32(args[3]), xy, args[4], 53u);
        let l = clamp(tone * (1.0 + (tex - 0.5) * args[5] * 1.5), 0.0, 1.0);
        if l < 0.5 {
            tone = pow(l / 0.5, 1.0 + args[1] * 2.0) * 0.5;
        } else {
            tone = 1.0 - pow((1.0 - l) / 0.5, 1.0 + args[2] * 2.0) * 0.5;
        }
    } else if mode == 5u {
        let phase = (xy.x * args[2] - xy.y * args[1]) * 0.5;
        let comb = abs(sin(phase * 3.141592653589793));
        let ink = (1.0 - tone) * (0.4 + args[3] * 1.2);
        tone = select(1.0, 0.0, comb < ink);
    } else if mode == 6u {
        let tau = 6.283185307179586;
        var screen = precise_sin_cos(xy.y / args[1] * tau).x * 0.5 + 0.5;
        if args[3] == 0.0 {
            let center = vec2<f32>(f32(image.width), f32(image.height)) / 2.0;
            screen = precise_sin_cos(length(xy - center) / args[1] * tau).x * 0.5 + 0.5;
        } else if args[3] == 1.0 {
            let sx = precise_sin_cos(xy.x / args[1] * tau).x;
            let sy = precise_sin_cos(xy.y / args[1] * tau).x;
            screen = sx * sy * 0.5 + 0.5;
        }
        let value = clamp((tone - 0.5) * (1.0 + args[2] * 4.0) + 0.5, 0.0, 1.0);
        tone = select(0.0, 1.0, screen < value);
    } else if mode == 7u {
        let sheet = select(0.35, 0.85, tone > 1.0 - args[1]);
        let grain = (value_noise(xy * 1.7, 811u) - 0.5) * args[2] * 0.5;
        tone = sheet + (q.r - 0.5) * 0.9 + grain;
    } else if mode == 8u {
        let below = (q.r - tone) * (4.0 + args[1] * 20.0);
        let flooded = max((0.25 - tone) * 6.0 * args[1], 0.0);
        tone = 1.0 - max(below, 0.0) - flooded;
    } else if mode == 9u {
        tone = tone * 0.65 + q.r * 0.35;
    } else if mode == 10u {
        let clump = fbm(xy / args[1], 4409u, 3u);
        tone += (clump - 0.5) * (args[2] * (1.0 - tone) + args[3] * tone) * 2.0;
    } else if mode == 11u {
        tone = select(0.0, 1.0, tone > 1.0 - args[1]);
    } else if mode == 12u {
        let tear = (fbm(xy / 24.0, 1723u, 3u) - 0.5) * 0.5;
        let edge = 1.0 - args[1] + tear;
        tone = (tone - edge) * (2.0 + args[2] * 30.0) + 0.5;
    } else if mode == 13u {
        let angle = fbm(xy / 40.0, 907u, 2u) * 6.283185307179586;
        var direction = vec2<f32>(cos(angle), sin(angle));
        direction /= max(length(direction), 0.000001);
        // Streak planes may be HDR; do not clip before the final tone mapping.
        return scalar_plane(stroke(pos, i32(args[1]), direction));
    } else if mode == 14u {
        let value = ((q.r - 0.5) * (0.5 + args[2] * 1.5) + 0.5) * (0.6 + args[1] * 0.8);
        let scale = clamp(value, 0.0, 1.0) / max(luminance(p), 0.0001);
        return vec4<f32>(clamp(p.rgb * scale, vec3<f32>(0.0), vec3<f32>(1.0)), p.a);
    }
    return scalar_plane(clamp(tone, 0.0, 1.0));
}
