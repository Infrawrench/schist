// Shared ABI. Coordinates passed to effect() and read helpers are relative
// to the FULL image, even when only an overlapping band is uploaded.
struct Image {
    width: u32,
    height: u32,
    first_row: u32,
    rows: u32,
}

@group(0) @binding(0) var<uniform> image: Image;
@group(0) @binding(1) var source: texture_2d_array<f32>;
@group(0) @binding(2) var<storage, read_write> destination: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read> args: array<f32>;

fn read_pixel(pos: vec2<i32>) -> vec4<f32> {
    let p = clamp(pos, vec2<i32>(0), vec2<i32>(i32(image.width) - 1, i32(image.height) - 1));
    let index = u32(p.y) * image.width + u32(p.x);
    let size = textureDimensions(source);
    let page = size.x * size.y;
    let local = index % page;
    return textureLoad(source, vec2<i32>(i32(local % size.x), i32(local / size.x)), i32(index / page), 0);
}
