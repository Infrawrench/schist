fn premul(p: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(p.rgb * p.a, p.a);
}

fn straight(p: vec4<f32>) -> vec4<f32> {
    if p.a > 0.000001 {
        return vec4<f32>(p.rgb / p.a, p.a);
    }
    return vec4<f32>(0.0, 0.0, 0.0, p.a);
}

fn luminance(p: vec4<f32>) -> f32 {
    return 0.299 * p.r + 0.587 * p.g + 0.114 * p.b;
}

// Rust f32::round rounds ties away from zero; WGSL round uses ties to even.
fn round_away(v: vec2<f32>) -> vec2<f32> {
    return sign(v) * floor(abs(v) + 0.5);
}

fn sample_premul(pos: vec2<f32>) -> vec4<f32> {
    return sample_premul_offset(vec2<i32>(0), pos);
}

fn sample_premul_offset(origin: vec2<i32>, pos: vec2<f32>) -> vec4<f32> {
    let lo = floor(pos);
    let t = pos - lo;
    let p = origin + vec2<i32>(lo);
    return premul(read_pixel(p)) * (1.0 - t.x) * (1.0 - t.y) + premul(read_pixel(p + vec2<i32>(1, 0))) * t.x * (1.0 - t.y) + premul(read_pixel(p + vec2<i32>(0, 1))) * (1.0 - t.x) * t.y + premul(read_pixel(p + vec2<i32>(1, 1))) * t.x * t.y;
}
