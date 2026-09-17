// args: amount, monochrome, Gaussian distribution.
fn raw_noise(x: u32, y: u32, c: u32) -> f32 {
    var h = (x * 0x9E3779B9u) ^ (y * 0x85EBCA6Bu) ^ (c * 0xC2B2AE35u);
    h ^= h >> 15u;
    h *= 0x2C1B3C6Du;
    h ^= h >> 12u;
    h *= 0x297A2D39u;
    h ^= h >> 15u;
    return (f32(h) / 4294967296.0) * 2.0 - 1.0;
}

fn noise(x: u32, y: u32, c: u32) -> f32 {
    let a = raw_noise(x, y, c);
    if args[2] == 0.0 {
        return a;
    }
    return (a + raw_noise(x ^ 0x5BD1E995u, y, c) + raw_noise(x, y ^ 0x1B873593u, c)) / 3.0 * 1.7;
}

fn effect(pos: vec2<i32>) -> vec4<f32> {
    var p = read_pixel(pos);
    for (var c = 0u; c < 3u; c++) {
        let channel = select(c, 0u, args[1] > 0.0);
        p[c] = clamp(p[c] + noise(u32(pos.x), u32(pos.y), channel) * args[0], 0.0, 1.0);
    }
    return p;
}
