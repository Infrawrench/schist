fn compute(i: u32) {
    let cell = u32(args[0]);
    let cols = (shape.width + cell - 1u) / cell;
    if args[1] == 0.0 || args[1] == 2.0 {
        let pixel = i / 4u;
        let c = i % 4u;
        let start = vec2(pixel % cols, pixel / cols) * cell;
        let end = min(start + vec2(cell), vec2(shape.width, shape.height));
        var sum = 0.0;
        for (var y = start.y; y < end.y; y++) {
            for (var x = start.x; x < end.x; x++) {
                let j = (y * shape.width + x) * 4u;
                sum += src[j + c] * select(src[j + 3u], 1.0, c == 3u || args[1] == 2.0);
            }
        }
        dst[i] = sum / f32((end.x - start.x) * (end.y - start.y));
    } else {
        let pixel = i / 4u;
        let c = i % 4u;
        let x = pixel % shape.width;
        let y = pixel / shape.width;
        let j = ((y / cell) * cols + x / cell) * 4u;
        let alpha = src[j + 3u];
        if args[1] == 3.0 {
            let height = 0.299 * src[j] + 0.587 * src[j + 1u] + 0.114 * src[j + 2u];
            let uv = vec2<f32>(f32(x % cell), f32(y % cell)) / f32(cell) - 0.5;
            let shade = 1.0 + (-(uv.x + uv.y)) * args[2] * (0.4 + height);
            dst[i] = select(clamp(src[j + c] * shade, 0.0, 1.0), alpha, c == 3u);
            return;
        }
        dst[i] = alpha;
        if c != 3u {
            dst[i] = select(0.0, src[j + c] / max(alpha, 0.000001), alpha > 0.000001);
        }
    }
}
