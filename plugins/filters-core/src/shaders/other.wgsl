fn effect(pos: vec2<i32>) -> vec4<f32> {
    let mode = u32(args[0]);
    let p = read_pixel(pos);
    let q = auxiliary_pixel(pos);
    if mode == 0u {
        let desired = luminance(p) + (q.rgb - luminance(q));
        return vec4<f32>(clamp(p.rgb + (desired - p.rgb) * args[1], vec3<f32>(0.0), vec3<f32>(1.0)), p.a);
    }
    if mode == 1u {
        if !((pos.x % 8 == 0 && pos.x > 0) || (pos.y % 8 == 0 && pos.y > 0)) {
            return p;
        }
        let mean = (p.rgb + read_pixel(pos - vec2<i32>(1, 0)).rgb + read_pixel(pos - vec2<i32>(0, 1)).rgb) / 3.0;
        return vec4<f32>(select(p.rgb, p.rgb + (mean - p.rgb) * 0.7, abs(p.rgb - mean) < vec3<f32>(0.1)), p.a);
    }
    if mode == 2u {
        let delta = p.rgb - q.rgb;
        let sharpened = clamp(p.rgb + delta * args[2], vec3<f32>(0.0), vec3<f32>(1.0));
        return vec4<f32>(select(p.rgb, sharpened, abs(delta) > vec3<f32>(args[1])), p.a);
    }
    if mode == 3u {
        var acc = vec4<f32>(0.0);
        var total = 0.0;
        for (var i = 4u; i + 1u < arrayLength(&args); i += 2u) {
            let value = premul(read_pixel(pos + vec2<i32>(i32(args[i]), i32(args[i + 1u]))));
            let l = luminance(value);
            var weight = 1.0;
            if args[3] != 0.0 {
                weight += l * l * l * args[1] * 8.0;
            } else if l > args[2] {
                weight += (l - args[2]) * (l - args[2]) * args[1] * 60.0;
            }
            acc += value * weight;
            total += weight;
        }
        return straight(acc / total);
    }
    if mode == 5u {
        var result = p;
        if args[1] > 0.0 {
            var depth = q.a;
            if args[1] == 2.0 {
                let i = 5u + (u32(pos.y) * image.width + u32(pos.x)) * 4u;
                depth = 0.299 * args[i] + 0.587 * args[i + 1u] + 0.114 * args[i + 2u];
            }
            if args[3] != 0.0 {
                depth = 1.0 - depth;
            }
            let keep = 1.0 - min(abs(depth - args[2]), 1.0);
            result += (q - result) * keep;
        }
        if args[4] > 0.0 {
            let noise = (value_noise(vec2<f32>(pos), 9173u) - 0.5) * args[4] * 0.35;
            result = vec4<f32>(clamp(result.rgb + noise, vec3<f32>(0.0), vec3<f32>(1.0)), result.a);
        }
        return result;
    }
    if mode == 6u {
        var acc = vec4<f32>(0.0);
        var count = 0.0;
        for (var t = -i32(args[1]); t <= i32(args[1]); t++) {
            acc += sample_plain(vec2<f32>(pos) + vec2<f32>(args[2], args[3]) * f32(t));
            count += 1.0;
        }
        return acc / count;
    }
    if mode == 7u {
        var result = p;
        let xy = vec2<f32>(pos) + 0.5;
        for (var c = 0u; c < 3u; c++) {
            let s = args[2u + c * 2u];
            let co = args[3u + c * 2u];
            let rotated = vec2<f32>(xy.x * co + xy.y * s, -xy.x * s + xy.y * co);
            let grid = round_away(rotated / args[1]) * args[1];
            let center = vec2<f32>(grid.x * co - grid.y * s, grid.x * s + grid.y * co);
            let level = 1.0 - read_pixel(vec2<i32>(center))[c];
            let radius = sqrt(max(level, 0.0)) * args[1] * 0.71;
            result[c] = select(1.0, 0.0, level >= 0.0 && length(xy - center) <= radius);
        }
        return result;
    }
    return p;
}
