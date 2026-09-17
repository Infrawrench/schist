fn curve(i: u32, value: f32) -> f32 {
    let base = u32(args[9u + i]);
    let n = u32(args[base]);
    let x = clamp(value, 0.0, 1.0) * f32(n - 1u);
    let j = u32(x + select(0.0, 0.5, i >= 3u));
    return args[base + 1u + min(j, n - 1u)];
}

fn compute(i: u32) {
    if i >= shape.width {
        return;
    }
    let b = i * 4u;
    let c = vec3(curve(0u, src[b]), curve(1u, src[b + 1u]), curve(2u, src[b + 2u]));
    for (var k = 0u; k < 3u; k++) {
        let m = k * 3u;
        dst[b + k] = curve(k + 3u, args[m] * c.r + args[m + 1u] * c.g + args[m + 2u] * c.b);
    }
    dst[b + 3u] = src[b + 3u];
}
