fn compute(i: u32) {
    let mode = u32(args[0]);
    let c = i % 4u;
    let base = i - c;
    if mode == 0u {
        dst[i] = select(src[i] * src[base + 3u], src[i], c == 3u);
        return;
    }
    if mode == 3u {
        var v = src[i];
        if c < 3u {
            v = 0.0;
            if src[base + 3u] > 0.000001 {
                v = src[i] / src[base + 3u];
            }
        }
        dst[i] = v;
        return;
    }
    let pixel = i / 4u;
    let x = pixel % shape.width;
    let y = pixel / shape.width;
    let r = i32(args[1]);
    var sum = 0.0;
    for (var k = -r; k <= r; k++) {
        var sx = i32(x);
        var sy = i32(y);
        if mode == 1u {
            sx = clamp(sx + k, 0, i32(shape.width) - 1);
        } else {
            sy = clamp(sy + k, 0, i32(shape.height) - 1);
        }
        sum += src[(u32(sy) * shape.width + u32(sx)) * 4u + c];
    }
    dst[i] = sum / f32(2 * r + 1);
}
