// Nearest two Voronoi centers, matching the CPU's deterministic visit order.
fn cell_of(pos: vec2<f32>, size: f32, seed: u32) -> vec4<f32> {
    let grid = floor(pos / size);
    var center = vec2<f32>(0.0);
    var best = 3.402823e38;
    var second = best;
    for (var dy = -1; dy <= 1; dy++) {
        for (var dx = -1; dx <= 1; dx++) {
            let c = grid + vec2<f32>(f32(dx), f32(dy));
            let jitter = vec2<f32>(value_noise(c * vec2<f32>(13.0, 7.0), seed), value_noise(c * vec2<f32>(7.0, 13.0), seed ^ 0x9e3779b9u));
            let point = (c + jitter) * size;
            let d = length(point - pos);
            if d < best {
                second = best;
                best = d;
                center = point;
            } else if d < second {
                second = d;
            }
        }
    }
    return vec4<f32>(center, best, second);
}

fn tile_shift(cell: vec2<f32>, span: f32, seed: u32) -> i32 {
    let v = (value_noise(cell, seed) - 0.5) * 2.0 * args[2] * span;
    if args[2] > 0.0 && abs(v) < 1.0 {
        return select(1, -1, v < 0.0);
    }
    return i32(rounded(v));
}

fn effect(pos: vec2<i32>) -> vec4<f32> {
    let mode = u32(args[0]);
    let p = read_pixel(pos);
    let q = auxiliary_pixel(pos);
    let xy = vec2<f32>(pos);
    let size = vec2<f32>(f32(image.width), f32(image.height));
    var color = p.rgb;
    if mode == 0u {
        let d = cell_of(xy, args[1], 3323u);
        let seam = min((d.w - d.z) / (args[1] * 0.35), 1.0);
        let base = 1.0 - seam;
        let crack = base * base * base;
        let e = cell_of(xy + 1.0, args[1], 3323u);
        // Rust signum returns +1 for positive zero.
        let lean = select(-1.0, 1.0, (e.w - e.z) - (d.w - d.z) >= 0.0);
        color *= 1.0 - crack * args[2] + crack * lean * args[3] * 0.35;
    } else if mode == 1u {
        var n = value_noise(xy, 13u) - 0.5;
        let kind = u32(args[3]);
        if kind == 1u {
            n = value_noise(xy / 2.0, 101u) - 0.5;
        } else if kind == 2u {
            n = max(value_noise(xy, 211u) - 0.85, 0.0) * 4.0;
        } else if kind == 3u {
            n = fbm(xy / 6.0, 307u, 3u) - 0.5;
        } else if kind == 4u {
            n = clamp((value_noise(xy, 401u) - 0.5) * 3.0, -0.5, 0.5);
        } else if kind == 5u {
            n = value_noise(xy / 4.0, 503u) - 0.5;
        } else if kind == 6u {
            n = -max(value_noise(xy, 601u) - 0.85, 0.0) * 4.0;
        } else if kind == 7u {
            n = value_noise(xy / vec2<f32>(6.0, 1.0), 701u) - 0.5;
        } else if kind == 8u {
            n = value_noise(xy / vec2<f32>(1.0, 6.0), 809u) - 0.5;
        } else if kind == 9u {
            let noise = value_noise(xy * 1.7, 907u) - 0.5;
            n = noise * noise * noise * 8.0;
        }
        color = clamp((color - 0.5) * (1.0 + args[2]) + 0.5, vec3<f32>(0.0), vec3<f32>(1.0)) + n * args[1];
    } else if mode == 2u {
        let d = cell_of(xy, args[1], 5051u);
        let seam = (d.w - d.z) / max(args[2] * 0.6, 0.5);
        if seam >= 1.0 {
            return p;
        }
        let desired = select(0.0, 1.0, args[3] >= 0.5);
        let pull = (1.0 - seam) * 0.9 * abs(args[3] - 0.5) * 2.0;
        color += (desired - color) * pull;
    } else if mode == 3u {
        let d = cell_of(xy, args[1], 8191u);
        color = read_pixel(vec2<i32>(d.xy)).rgb;
        let seam = (d.w - d.z) / max(args[2] * 0.35, 0.3);
        if seam < 1.0 {
            return vec4<f32>(color * (1.0 - (1.0 - seam) * 0.95), p.a);
        } else if args[3] > 0.0 {
            let center = size / 2.0;
            let distance = length(xy - center) / max(length(center), 1.0);
            color *= 1.0 + (1.0 - distance) * args[3];
        } else {
            return vec4<f32>(color, p.a);
        }
    } else if mode == 4u {
        let light = vec2<f32>(args[1], args[2]);
        let at = xy + 0.5;
        let span = max(size.x, size.y);
        let d = length(at - light) / span;
        var add = (0.35 / (1.0 + d * d * 900.0) + 0.12 / (1.0 + d * d * 40.0)) * args[3];
        for (var i = 4u; i + 2u < arrayLength(&args); i += 3u) {
            let ghost = light + (size / 2.0 - light) * 2.0 * args[i];
            let gd = length(at - ghost) / span;
            add += (0.06 * args[i + 2u] / (1.0 + gd * gd / (args[i + 1u] * args[i + 1u]))) * args[3];
        }
        color += vec3<f32>(add, add * 0.95, add * 0.85);
    } else if mode == 5u {
        let gx = read_pixel(pos + vec2<i32>(1, 0)).r - read_pixel(pos - vec2<i32>(1, 0)).r;
        let gy = read_pixel(pos + vec2<i32>(0, 1)).r - read_pixel(pos - vec2<i32>(0, 1)).r;
        let normal = normalize(vec3<f32>(-gx * (8.0 * args[5]), -gy * (8.0 * args[5]), 1.0));
        var light = vec3<f32>(args[7], args[8], 0.8);
        var falloff = 1.0;
        if args[1] != 2.0 {
            let delta = vec2<f32>(args[9], args[10]) - xy;
            let d = length(delta);
            falloff = clamp(1.0 - d / max(args[6], 1.0), 0.0, 1.0);
            if args[1] == 0.0 {
                falloff *= falloff;
            }
            light = vec3<f32>(delta / max(d, 0.000001), select(0.5, 1.2, args[1] == 0.0));
        }
        light /= max(length(light), 0.000001);
        let diffuse = max(dot(normal, light), 0.0);
        let half_vector = vec3<f32>(light.xy, light.z + 1.0);
        let spec = pow(max(dot(normal, half_vector) / max(length(half_vector), 0.000001), 0.0), 4.0 + args[4] * 60.0) * args[4];
        let lit = args[3] + args[2] * falloff * (diffuse + spec);
        return vec4<f32>(clamp(q.rgb * lit, vec3<f32>(0.0), vec3<f32>(1.0)), q.a);
    } else if mode == 6u {
        let opposite = size - 1.0 - xy;
        let edge = min(min(xy.x, xy.y), min(opposite.x, opposite.y));
        let t = edge / args[2];
        if t >= 1.0 {
            return p;
        }
        var facing = vec2<f32>(0.0, 1.0);
        if edge == xy.x {
            facing = vec2<f32>(-1.0, 0.0);
        } else if edge == opposite.x {
            facing = vec2<f32>(1.0, 0.0);
        } else if edge == xy.y {
            facing = vec2<f32>(0.0, -1.0);
        }
        var height = 1.0;
        var slope = 0.0;
        if args[1] == 1.0 {
            height = t;
            slope = 1.0;
        } else if args[1] == 2.0 {
            height = 0.15;
            if t > 0.75 {
                height = (t - 0.75) * 4.0;
                slope = 1.0;
            }
        } else if args[1] >= 3.0 {
            height = sin(t * 3.141592653589793);
            slope = cos(t * 3.141592653589793);
            if args[1] > 3.0 {
                let bead = sin(t * 18.0) * 0.12;
                height += bead;
                slope += bead * 3.0;
            }
        }
        let lit = 1.0 + slope * (facing.x + facing.y) * -0.5 * args[4];
        let shade = (args[3] + 0.35) * lit + height * 0.12 * args[4];
        return vec4<f32>(clamp(vec3<f32>(shade, shade * 0.94, shade * 0.86), vec3<f32>(0.0), vec3<f32>(1.0)), max(p.a, 1.0));
    } else if mode == 7u {
        let c = clamp((p.r - 0.5) * (1.0 + args[1] * 3.0) + 0.5, 0.0, 1.0);
        return scalar_plane(select(c, 1.0 - c, args[2] != 0.0));
    } else if mode == 8u {
        return vec4<f32>(p.rgb, q.a);
    } else if mode == 9u {
        let gradient = tone_gradient(pos) * 4.0;
        let normal = normalize(vec3<f32>(-gradient * args[1], 1.0));
        return vec4<f32>(clamp(normal * 0.5 + 0.5, vec3<f32>(0.0), vec3<f32>(1.0)), q.a);
    } else if mode == 10u {
        let wx = array<f32, 9>(-1.0, 0.0, 1.0, -2.0, 0.0, 2.0, -1.0, 0.0, 1.0);
        let wy = array<f32, 9>(-1.0, -2.0, -1.0, 0.0, 0.0, 0.0, 1.0, 2.0, 1.0);
        var gradient = vec2<f32>(0.0);
        for (var i = 0; i < 9; i++) {
            let l = luminance(read_pixel(pos + vec2<i32>(i % 3 - 1, i / 3 - 1)));
            gradient += l * vec2<f32>(wx[i], wy[i]);
        }
        return scalar_plane(length(gradient));
    } else if mode == 11u {
        color = color / max(luminance(p), 0.0001) * clamp(q.r * args[1], 0.0, 1.0);
    } else if mode == 12u {
        color = select(color, 1.0 - color, color > vec3<f32>(0.5)) * 2.0;
    } else if mode == 13u {
        for (var back = 1; back < i32(args[1]); back++) {
            let sx = pos.x + select(-back, back, args[2] != 0.0);
            if sx < 0 || sx >= i32(image.width) {
                break;
            }
            let at = vec2<i32>(sx, pos.y);
            if auxiliary_pixel(at).r < 0.25 {
                continue;
            }
            let seed_y = select(pos.y, pos.y / 3 * 3, args[3] != 0.0);
            let run = value_noise(vec2<f32>(f32(sx), f32(seed_y)), 613u) * args[1];
            if f32(back) > run {
                continue;
            }
            let k = 1.0 - f32(back) / args[1];
            color = color * (1.0 - k) + read_pixel(at).rgb * k;
        }
        return vec4<f32>(color, p.a);
    } else if mode == 14u {
        let count = u32(args[1]);
        let span = vec2<u32>((image.width + count - 1u) / count, (image.height + count - 1u) / count);
        var result = p;
        if args[3] == 0.0 {
            result = vec4<f32>(0.0);
        } else if args[3] == 1.0 {
            result = vec4<f32>(args[7], args[8], args[9], 1.0);
        } else if args[3] == 2.0 {
            result = vec4<f32>(args[4], args[5], args[6], 1.0);
        } else if args[3] == 3.0 {
            result = vec4<f32>(1.0 - p.rgb, p.a);
        }
        // Gather overlapping shifted cells in CPU painting order. Only cells
        // within the maximum shift can contribute, so there are no write races.
        let reach = vec2<i32>(ceil(abs(args[2]) * vec2<f32>(span))) + vec2<i32>(1);
        let low = max(pos - reach, vec2<i32>(0)) / vec2<i32>(span);
        let high = min(pos + reach, vec2<i32>(i32(image.width) - 1, i32(image.height) - 1)) / vec2<i32>(span);
        for (var ty = low.y; ty <= high.y; ty++) {
            for (var tx = low.x; tx <= high.x; tx++) {
                let cell = vec2<f32>(f32(tx), f32(ty));
                let delta = vec2<i32>(tile_shift(cell, f32(span.x), 17u), tile_shift(cell, f32(span.y), 71u));
                let source_pos = pos - delta;
                let start = vec2<i32>(tx, ty) * vec2<i32>(span);
                let end = min(start + vec2<i32>(span), vec2<i32>(i32(image.width), i32(image.height)));
                if all(source_pos >= start) && all(source_pos < end) {
                    result = read_pixel(source_pos);
                }
            }
        }
        return result;
    } else if mode == 15u {
        var picked = p;
        if args[2] == 3.0 {
            var best = 3.402823e38;
            for (var k = 0u; k < 8u; k++) {
                let sample = read_pixel(pos + vec2<i32>(i32(args[3u + k * 2u]), i32(args[4u + k * 2u])));
                let delta = abs(sample.rgb - p.rgb);
                let d = max(delta.r, max(delta.g, delta.b));
                if d < best {
                    best = d;
                    picked = sample;
                }
            }
        } else {
            let delta = (vec2<f32>(value_noise(xy, 5u), value_noise(xy, 9u)) - 0.5) * 2.0 * args[1];
            picked = read_pixel(pos + vec2<i32>(delta));
        }
        if (args[2] == 1.0 && luminance(picked) > luminance(p)) || (args[2] == 2.0 && luminance(picked) < luminance(p)) {
            return p;
        }
        return picked;
    }
    return vec4<f32>(clamp(color, vec3<f32>(0.0), vec3<f32>(1.0)), p.a);
}
