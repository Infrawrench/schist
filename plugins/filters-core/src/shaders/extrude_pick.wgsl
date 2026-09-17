fn compute(i: u32) {
    let size = u32(args[0]);
    let cols = (shape.width + size - 1u) / size;
    let block = i / (size * size);
    let bx = block % cols;
    let by = block / cols;
    let x = i % size;
    let y = i / size % size;
    let center = min(vec2<u32>(bx * size + size / 2u, by * size + size / 2u), vec2<u32>(shape.width - 1u, shape.height - 1u));
    let j = (center.y * shape.width + center.x) * 4u;
    var level = 0.299 * src[j] + 0.587 * src[j + 1u] + 0.114 * src[j + 2u];
    if args[2] >= 0.5 {
        level = value_noise(vec2<f32>(f32(bx), f32(by)), 4177u);
    }
    let scale = 1.0 + level * args[1] / 100.0;
    let mid = vec2<f32>(f32(shape.width), f32(shape.height)) / 2.0;
    let out = mid + (vec2<f32>(f32(bx * size + x), f32(by * size + y)) - mid) * scale;
    if all(out >= vec2<f32>(0.0)) && all(out < vec2<f32>(f32(shape.width), f32(shape.height))) {
        atomicMax(&dst[u32(out.y) * shape.width + u32(out.x)], bitcast<u32>(f32(i + 1u)));
    }
}
