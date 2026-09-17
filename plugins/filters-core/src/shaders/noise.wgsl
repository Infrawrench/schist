fn hash(x: i32, y: i32, seed: u32) -> f32 {
    var n = bitcast<u32>(x) * 0x9E3779B1u + bitcast<u32>(y) * 0x85EBCA6Bu + seed * 0xC2B2AE35u;
    n ^= n >> 15u;
    n *= 0x2545F491u;
    n ^= n >> 13u;
    return f32(n & 65535u) / 65535.0;
}

fn value_noise(pos: vec2<f32>, seed: u32) -> f32 {
    let lo = floor(pos);
    let t = pos - lo;
    let s = t * t * (vec2<f32>(3.0) - 2.0 * t);
    let p = vec2<i32>(lo);
    let a = hash(p.x, p.y, seed);
    let b = hash(p.x + 1, p.y, seed);
    let c = hash(p.x, p.y + 1, seed);
    let d = hash(p.x + 1, p.y + 1, seed);
    let top = a + (b - a) * s.x;
    let bottom = c + (d - c) * s.x;
    return top + (bottom - top) * s.y;
}

fn fbm(pos: vec2<f32>, seed: u32, octaves: u32) -> f32 {
    var sum = 0.0;
    var amp = 0.5;
    var freq = 1.0;
    var norm = 0.0;
    for (var o = 0u; o < octaves; o++) {
        sum += value_noise(pos * freq, seed + o * 7919u) * amp;
        norm += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    return sum / max(norm, 0.000001);
}
