fn compute(i: u32) {
    let pixel = i / shape.channels;
    let c = i % shape.channels;
    if aux[pixel * 2u] == 0.0 {
        dst[i] = src[i];
        return;
    }
    let x = i32(pixel % shape.width);
    let y = i32(pixel / shape.width);
    let offsets = array<vec2<i32>, 4>(vec2(1, 0), vec2(-1, 0), vec2(0, 1), vec2(0, -1));
    var sum = 0.0;
    var n = 0.0;
    for (var k = 0u; k < 4u; k++) {
        let p = vec2(x, y) + offsets[k];
        if p.x < 0 || p.y < 0 || p.x >= i32(shape.width) || p.y >= i32(shape.height) {
            continue;
        }
        let j = u32(p.y) * shape.width + u32(p.x);
        if args[0] != 0.0 && aux[j * 2u] == 0.0 && aux[j * 2u + 1u] == 0.0 {
            continue;
        }
        sum += src[j * shape.channels + c];
        n += 1.0;
    }
    dst[i] = select(src[i], sum / max(n, 1.0), n > 0.0);
}
