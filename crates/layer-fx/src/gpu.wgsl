fn rgba(index: u32) -> vec4<f32> {
    let b = index * 4u;
    return vec4(src[b], src[b + 1u], src[b + 2u], src[b + 3u]);
}

fn other_rgba(index: u32) -> vec4<f32> {
    let b = index * 4u;
    return vec4(aux[b], aux[b + 1u], aux[b + 2u], aux[b + 3u]);
}

fn store_rgba(index: u32, value: vec4<f32>) {
    for (var c = 0u; c < 4u; c++) {
        dst[index * 4u + c] = value[c];
    }
}

fn compute(i: u32) {
    let mode = u32(args[0]);
    if mode == 0u {
        // Colour an alpha plane, applying spread only after preparation.
        var a = src[i];
        if args[6] != 0.0 {
            a = 1.0;
        }
        if args[5] > 0.0 {
            a = min(a / max(1.0 - clamp(args[5], 0.0, 0.99), 0.01), 1.0);
        }
        store_rgba(i, vec4(args[1], args[2], args[3], a * args[4]));
        return;
    }
    if mode == 1u {
        let p = rgba(i);
        let c = u32(args[1]);
        dst[i] = select(p[c] * p.a, p.a, c == 3u);
        return;
    }
    if mode == 2u {
        var p = other_rgba(i);
        p[u32(args[1])] = src[i];
        store_rgba(i, p);
        return;
    }
    if mode == 3u {
        var p = rgba(i);
        if p.a > 1.1920929e-7 {
            p = vec4(p.rgb * (1.0 / p.a), p.a);
        }
        if args[1] != 0.0 {
            p.a = aux[i * 4u + 3u];
        }
        store_rgba(i, p);
        return;
    }
    if mode == 4u {
        dst[i] = 1.0 - src[i];
        return;
    }
    if mode == 5u {
        let d = abs(src[i] - aux[i]);
        dst[i] = select(d, 1.0 - d, args[1] != 0.0);
        return;
    }
    if mode == 6u {
        var up = 1.0;
        if args[3] == 0.0 {
            up = clamp((src[i] - args[1]) / 0.5 + 0.5, 0.0, 1.0);
        }
        let down = clamp((args[2] - src[i]) / 0.5 + 0.5, 0.0, 1.0);
        dst[i] = select(clamp(up * down, 0.0, 1.0), 0.0, args[3] == 0.0 && args[2] <= args[1]);
        return;
    }
    if mode == 7u {
        let x = i32(args[2]) + i32(i % shape.width);
        let y = i32(args[3]) + i32(i / shape.width);
        store_rgba(i, blend_px(u32(args[1]), rgba(i), other_rgba(i), x, y));
        return;
    }
    if mode == 8u {
        var p = rgba(i);
        p.a *= args[1];
        if args[2] == 1.0 {
            p.a *= aux[i];
        } else if args[2] == 2.0 {
            p.a *= 1.0 - aux[i];
        }
        store_rgba(i, p);
        return;
    }
    if mode == 9u {
        let p = vec2(f32(i % shape.width), f32(i / shape.width)) + vec2(args[1], args[2]);
        var t = (p.x * args[3] + p.y * args[4]) / (2.0 * args[5]) + 0.5;
        if args[6] != 0.0 {
            t = length(p) / args[5];
        }
        t = clamp(t, 0.0, 1.0);
        if args[7] != 0.0 {
            t = 1.0 - t;
        }
        let start_color = vec4(args[8], args[9], args[10], args[11]);
        let end_color = vec4(args[12], args[13], args[14], args[15]);
        store_rgba(i, start_color + (end_color - start_color) * t);
        return;
    }
    if mode == 10u {
        let x = i % shape.width;
        let y = i / shape.width;
        let left = src[i - min(x, 1u)];
        let right = src[min(i + 1u, y * shape.width + shape.width - 1u)];
        let up = src[i - min(y, 1u) * shape.width];
        let down = src[min(i + shape.width, (shape.height - 1u) * shape.width + x)];
        let nx = (left - right) * args[1] * args[2];
        let ny = (up - down) * args[1] * args[2];
        let len = sqrt(nx * nx + ny * ny + 1.0);
        let dot = (nx * args[3] + ny * args[4] + args[5]) / len;
        let shade = (dot - args[5]) / max(1.0 - args[5], 0.001);
        var gate = 1.0;
        if args[6] == 0.0 {
            gate = 1.0 - aux[i];
        } else if args[6] == 1.0 {
            gate = aux[i];
        }
        var a = 0.0;
        if args[7] == 0.0 && shade > 0.0 {
            a = min(shade, 1.0) * gate;
        } else if args[7] == 1.0 && shade <= 0.0 {
            a = min(-shade, 1.0) * gate;
        }
        dst[i] = a;
    }
}
