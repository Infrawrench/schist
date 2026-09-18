fn effect(pos: vec2<i32>) -> vec4<f32> {
    let p = read_pixel(pos);
    let mode = u32(args[0]);
    if mode == 0u {
        let d = p.rgb - auxiliary_pixel(pos).rgb;
        if max(max(abs(d.r), abs(d.g)), abs(d.b)) < 0.03 {
            return p;
        }
        return vec4<f32>(clamp(p.rgb + d * 1.5, vec3<f32>(0.0), vec3<f32>(1.0)), p.a);
    }
    if mode == 1u {
        let bright = max(luminance(p) - args[1], 0.0) / max(1.0 - args[1], 0.001);
        return vec4<f32>(vec3<f32>(bright), 1.0);
    }
    if mode == 2u {
        let grain = (value_noise(vec2<f32>(pos), 5237u) - 0.5) * args[1] * 0.35;
        let lift = clamp(auxiliary_pixel(pos).r * (0.6 + args[2] * 2.0) + grain, 0.0, 1.0);
        let color = vec3<f32>(args[3], args[4], args[5]);
        return vec4<f32>(clamp(color - (color - p.rgb) * (1.0 - lift), vec3<f32>(0.0), vec3<f32>(1.0)), p.a);
    }
    if mode == 3u {
        let m = u32(args[1]);
        let lightness = m == 1u || m == 3u;
        if m < 2u {
            let hi = max(max(p.r, p.g), p.b);
            let lo = min(min(p.r, p.g), p.b);
            let c = hi - lo;
            var hue = 0.0;
            if c > 0.000001 {
                if hi == p.r {
                    hue = ((p.g - p.b) / c) % 6.0 / 6.0;
                } else if hi == p.g {
                    hue = ((p.b - p.r) / c + 2.0) / 6.0;
                } else {
                    hue = ((p.r - p.g) / c + 4.0) / 6.0;
                }
            }
            if hue < 0.0 {
                hue += 1.0;
            }
            if lightness {
                let l = (hi + lo) / 2.0;
                var sat = 0.0;
                if l > 0.0 && l < 1.0 {
                    sat = c / (1.0 - abs(2.0 * l - 1.0));
                }
                return vec4<f32>(hue, clamp(sat, 0.0, 1.0), l, p.a);
            }
            var sat = 0.0;
            if hi > 0.0 {
                sat = c / hi;
            }
            return vec4<f32>(hue, sat, hi, p.a);
        }
        var c = p.b * p.g;
        var offset = p.b - c;
        if lightness {
            c = (1.0 - abs(2.0 * p.b - 1.0)) * p.g;
            offset = p.b - c / 2.0;
        }
        let hp = (p.r - floor(p.r)) * 6.0;
        let x = c * (1.0 - abs(hp % 2.0 - 1.0));
        var rgb = vec3<f32>(c, 0.0, x);
        switch u32(hp) {
            case 0u: {
                rgb = vec3<f32>(c, x, 0.0);
            }
            case 1u: {
                rgb = vec3<f32>(x, c, 0.0);
            }
            case 2u: {
                rgb = vec3<f32>(0.0, c, x);
            }
            case 3u: {
                rgb = vec3<f32>(0.0, x, c);
            }
            case 4u: {
                rgb = vec3<f32>(x, 0.0, c);
            }
            default: {
            }
        }
        return vec4<f32>(clamp(rgb + offset, vec3<f32>(0.0), vec3<f32>(1.0)), p.a);
    }
    if mode == 4u {
        if (pos.y % 2 == 1) != (args[1] < 0.5) {
            return p;
        }
        let above = read_pixel(pos - vec2<i32>(0, 1));
        if args[2] < 0.5 {
            return (above + read_pixel(pos + vec2<i32>(0, 1))) / 2.0;
        }
        return above;
    }
    if mode == 5u {
        let rgb = 0.0627 + p.rgb * (0.9216 - 0.0627);
        let l = luminance(vec4<f32>(rgb, 1.0));
        return vec4<f32>(l + clamp(rgb - l, vec3<f32>(-0.32), vec3<f32>(0.32)), p.a);
    }
    var sum = vec3<f32>(0.0);
    var correction = vec3<f32>(0.0);
    for (var i = 0u; i < u32(args[1]); i++) {
        let value = vec3<f32>(aux[i * 3u], aux[i * 3u + 1u], aux[i * 3u + 2u]) - correction;
        let next = sum + value;
        correction = (next - sum) - value;
        sum = next;
    }
    return vec4<f32>(sum / f32(image.width * image.height), p.a);
}
