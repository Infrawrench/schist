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
