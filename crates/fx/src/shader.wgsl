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

// Native GPU trig can lose enough precision to move a sample or flip a binary
// screen at its extrema. Reduce to [-pi/4, pi/4] with a split pi/2, then evaluate
// sine and cosine directly so values near their peaks round to exactly +/-1.
fn precise_sin_cos(angle: f32) -> vec2<f32> {
    if abs(angle) > 65536.0 {
        return vec2<f32>(sin(angle), cos(angle));
    }
    let quadrant = i32(round(angle * 0.6366197723675814));
    let n = f32(quadrant);
    // Each leading word has at most eight significant bits. Its product with
    // the bounded quadrant is exact even when a backend does not fuse fma.
    let high = fma(-n, 1.5703125, angle);
    let middle = fma(-n, 0.0004825592041015625, high);
    let low = fma(-n, 0.00000126659870147705078125, middle);
    let x = fma(-n, 9.920935796805405e-10, low);
    let square = x * x;
    var s = 1.0 / 6227020800.0;
    s = fma(s, square, -1.0 / 39916800.0);
    s = fma(s, square, 1.0 / 362880.0);
    s = fma(s, square, -1.0 / 5040.0);
    s = fma(s, square, 1.0 / 120.0);
    s = fma(s, square, -1.0 / 6.0);
    s = fma(x * square, s, x);
    var c = 1.0 / 479001600.0;
    c = fma(c, square, -1.0 / 3628800.0);
    c = fma(c, square, 1.0 / 40320.0);
    c = fma(c, square, -1.0 / 720.0);
    c = fma(c, square, 1.0 / 24.0);
    c = fma(c, square, -0.5);
    c = fma(c, square, 1.0);
    switch quadrant & 3 {
        case 1: {
            return vec2<f32>(c, -s);
        }
        case 2: {
            return vec2<f32>(-s, -c);
        }
        case 3: {
            return vec2<f32>(-c, s);
        }
        default: {
            return vec2<f32>(s, c);
        }
    }
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
