// Shared ABI. Coordinates passed to effect() and read helpers are relative
// to the FULL image, even when only an overlapping band is uploaded.
struct Image {
    width: u32,
    height: u32,
    first_row: u32,
    rows: u32,
}

@group(0) @binding(0) var<uniform> image: Image;
@group(0) @binding(1) var<storage, read> source: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> destination: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read> args: array<f32>;

fn read_pixel(pos: vec2<i32>) -> vec4<f32> {
    let p = clamp(pos, vec2<i32>(0), vec2<i32>(i32(image.width) - 1, i32(image.height) - 1));
    // Halo rows are discarded; their own reads may reach past the band.
    let row = clamp(u32(p.y), image.first_row, image.first_row + image.rows - 1u);
    return source[(row - image.first_row) * image.width + u32(p.x)];
}

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
    return premul(read_pixel(p)) * (1.0 - t.x) * (1.0 - t.y)
        + premul(read_pixel(p + vec2<i32>(1, 0))) * t.x * (1.0 - t.y)
        + premul(read_pixel(p + vec2<i32>(0, 1))) * (1.0 - t.x) * t.y
        + premul(read_pixel(p + vec2<i32>(1, 1))) * t.x * t.y;
}

@compute @workgroup_size(16, 16)
fn run_effect(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= image.width || id.y >= image.rows {
        return;
    }
    destination[id.y * image.width + id.x] = effect(vec2<i32>(i32(id.x), i32(id.y + image.first_row)));
}
