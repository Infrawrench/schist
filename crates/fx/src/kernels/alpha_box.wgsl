// One invocation per row/column. Linear work and the reference's sum order.
fn compute(line: u32) {
    let vertical = args[1] != 0.0;
    let outer = select(shape.height, shape.width, vertical);
    let inner = select(shape.width, shape.height, vertical);
    if line >= outer || inner == 0u {
        return;
    }
    let stride = select(1u, shape.width, vertical);
    let step = select(shape.width, 1u, vertical);
    let base = line * step;
    let r = u32(args[0]);
    let window = f32(r * 2u + 1u);
    var acc = 0.0;
    if args[2] != 0.0 {
        acc = src[base] * f32(r + 1u);
        for (var k = 0u; k < min(r, inner); k++) {
            acc += src[base + k * stride];
        }
        let norm = 1.0 / window;
        for (var i = 0u; i < inner; i++) {
            let add = src[base + min(i + r, inner - 1u) * stride];
            var sub = src[base];
            if i > r {
                sub = src[base + (i - r - 1u) * stride];
            }
            acc += add - sub;
            dst[base + i * stride] = acc * norm;
        }
    } else {
        for (var k = 0u; k <= r; k++) {
            acc += src[base + min(k, inner - 1u) * stride];
        }
        acc += src[base] * f32(r);
        for (var i = 0u; i < inner; i++) {
            dst[base + i * stride] = acc / window;
            let add = src[base + min(i + r + 1u, inner - 1u) * stride];
            let sub = src[base + u32(max(i32(i) - i32(r), 0)) * stride];
            acc += add - sub;
        }
    }
}
