fn trunc_fract(p: vec2<f32>) -> vec2<f32> {
    return p - trunc(p);
}

fn surface(kind: u32, p: vec2<f32>, scale: f32, seed: u32) -> f32 {
    let uv = p / max(scale, 1.0);
    let tau = 6.283185307179586;
    if kind == 0u {
        let weave = (sin(uv.x * tau) + sin(uv.y * tau)) * 0.25 + 0.5;
        return weave * 0.35 + fbm(uv * 2.0, seed, 3u) * 0.45 + value_noise(p, seed) * 0.2;
    }
    if kind == 1u {
        return fbm(uv, seed, 3u) * 0.6 + value_noise(p, seed ^ 0x5bd1u) * 0.4;
    }
    if kind == 2u {
        let warp = clamp(abs(sin(uv.x * tau)) * 0.6 + abs(sin(uv.y * tau * 0.5)) * 0.4, 0.0, 1.0);
        return warp * 0.65 + value_noise(p * vec2(1.3, 0.9), seed) * 0.35;
    }
    let shift = select(0.5, 0.0, i32(floor(uv.y)) % 2 == 0);
    let c = trunc_fract(uv + vec2(shift, 0.0));
    let mortar = select(0.85, 0.25, c.x < 0.06 || c.y < 0.12);
    return mortar * 0.8 + value_noise(p * 0.5, seed) * 0.2;
}

fn auxiliary_pixel(pos: vec2<i32>) -> vec4<f32> {
    let p = clamp(pos, vec2<i32>(0), vec2<i32>(i32(image.width) - 1, i32(image.height) - 1));
    let i = (u32(p.y) * image.width + u32(p.x)) * 4u;
    return vec4<f32>(aux[i], aux[i + 1u], aux[i + 2u], aux[i + 3u]);
}

// Straight bilinear sampling, preserving the CPU gallery's accumulation order.
fn sample_plain(pos: vec2<f32>) -> vec4<f32> {
    let lo = floor(pos);
    let t = pos - lo;
    let p = vec2<i32>(lo);
    let top = read_pixel(p) * (1.0 - t.x) + read_pixel(p + vec2<i32>(1, 0)) * t.x;
    let bottom = read_pixel(p + vec2<i32>(0, 1)) * (1.0 - t.x) + read_pixel(p + vec2<i32>(1, 1)) * t.x;
    return top * (1.0 - t.y) + bottom * t.y;
}

fn tone_gradient(pos: vec2<i32>) -> vec2<f32> {
    let nw = read_pixel(pos + vec2<i32>(-1, -1)).r;
    let n = read_pixel(pos + vec2<i32>(0, -1)).r;
    let ne = read_pixel(pos + vec2<i32>(1, -1)).r;
    let w = read_pixel(pos + vec2<i32>(-1, 0)).r;
    let e = read_pixel(pos + vec2<i32>(1, 0)).r;
    let sw = read_pixel(pos + vec2<i32>(-1, 1)).r;
    let s = read_pixel(pos + vec2<i32>(0, 1)).r;
    let se = read_pixel(pos + vec2<i32>(1, 1)).r;
    return vec2<f32>(ne + 2.0 * e + se - nw - 2.0 * w - sw, sw + 2.0 * s + se - nw - 2.0 * n - ne) / 4.0;
}

fn scalar_plane(value: f32) -> vec4<f32> {
    return vec4<f32>(value, value, value, 1.0);
}

fn tint_tone(original: vec4<f32>, value: f32, tint: f32) -> vec4<f32> {
    let v = clamp(value, 0.0, 1.0);
    if tint <= 0.0 {
        return vec4<f32>(vec3<f32>(v), original.a);
    }
    let color = clamp(original.rgb / max(luminance(original), 0.0001) * v, vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(v + (color - v) * tint, original.a);
}

fn stroke(pos: vec2<i32>, steps: i32, direction: vec2<f32>) -> f32 {
    var sum = 0.0;
    var count = 0.0;
    for (var t = -steps; t <= steps; t++) {
        sum += sample_plain(vec2<f32>(pos) + direction * f32(t)).r;
        count += 1.0;
    }
    return sum / count;
}

fn rounded(value: f32) -> f32 {
    return sign(value) * floor(abs(value) + 0.5);
}

fn flat_color(p: vec4<f32>, levels: f32, chroma_step: f32) -> vec4<f32> {
    let l = luminance(p);
    let n = max(levels, 2.0) - 1.0;
    let tone = rounded(l * n) / n;
    let step = max(chroma_step, 0.001);
    let a = rounded((p.r - l) / step) * step;
    let b = rounded((p.b - l) / step) * step;
    let r = tone + a;
    let bl = tone + b;
    let g = (tone - 0.299 * r - 0.114 * bl) / 0.587;
    return vec4<f32>(clamp(vec3<f32>(r, g, bl), vec3<f32>(0.0), vec3<f32>(1.0)), p.a);
}
